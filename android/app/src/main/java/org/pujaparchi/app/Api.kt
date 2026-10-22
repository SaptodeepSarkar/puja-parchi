package org.pujaparchi.app

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.asRequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.net.URLEncoder
import java.util.concurrent.TimeUnit

/**
 * The Rust server's HTTP contract, mirroring `app/src/api.ts`.
 *
 * Plain http on purpose: a native app is not bound by the browser's
 * mic-needs-https rule, so the phone talks to `http://<laptop>:8080` and the
 * whole certificate story disappears.
 */

class ApiException(val status: Int, message: String) : Exception(message)

data class Field(
    val value: String?,
    val source: String,
    val confidence: Double,
    val evidence: List<String>,
    val alternates: List<String>,
)

data class Draft(
    val id: String,
    val book: Long,
    val receipt: Field,
    val name: Field,
    val amount: Field,
    val mode: Field,
    val status: String,
    val rawText: String,
    val visionNote: String,
    val transcript: String,
    val photoUrl: String,
    val audioUrl: String?,
    val warnings: List<String>,
    val createdAt: String,
    val committedRow: Int?,
)

data class Totals(val cash: Long, val online: Long, val total: Long)
data class ConfirmResult(val draft: Draft, val row: Int, val totals: Totals)
data class BookInfo(val book: Long?, val recent: List<Long>)

data class Health(
    val version: String,
    val vision: String,
    val stt: String,
    val visionReady: Boolean,
    val sttReady: Boolean,
    val ledgerOk: Boolean,
    val ledgerPath: String,
    val activeBook: Long?,
)

data class Stats(
    val todayTotal: Long,
    val todayReceipts: Long,
    val ledgerCash: Long,
    val ledgerOnline: Long,
    val ledgerTotal: Long,
    val ledgerRow: Long,
)

class Api(private val store: Store) {

    private val http = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        // A capture waits for the vision model (up to 45 s on the server) and
        // speech-to-text (20 s), so this must outlast both.
        .readTimeout(150, TimeUnit.SECONDS)
        .writeTimeout(60, TimeUnit.SECONDS)
        .build()

    private fun req(path: String): Request.Builder = Request.Builder().url(store.base + path).apply {
        if (store.pin.isNotEmpty()) header("x-pin", store.pin)
    }

    private fun exec(builder: Request.Builder): String {
        http.newCall(builder.build()).execute().use { res ->
            val text = res.body?.string().orEmpty()
            if (!res.isSuccessful) throw ApiException(res.code, messageOf(text, res.code))
            return text
        }
    }

    /** axum answers errors as {"error": "..."}; 413 and friends come back as plain text. */
    private fun messageOf(text: String, code: Int): String {
        val t = text.trim()
        if (t.startsWith("{")) {
            try {
                val msg = JSONObject(t).optString("error", "")
                if (msg.isNotEmpty()) return msg
            } catch (_: Exception) {
            }
        }
        return if (t.isNotEmpty() && t.length < 200 && !t.startsWith("<")) t else "Request failed ($code)"
    }

    private fun get(path: String) = exec(req(path).get())
    private fun post(path: String, body: JSONObject? = null) =
        exec(req(path).post(body?.toString()?.toRequestBody(JSON) ?: EMPTY))

    private fun patch(path: String, body: JSONObject) = exec(req(path).patch(body.toString().toRequestBody(JSON)))

    private companion object {
        val JSON = "application/json; charset=utf-8".toMediaType()
        val EMPTY = ByteArray(0).toRequestBody(null)
    }

    // ---------------------------------------------------------------- parsing

    private fun field(o: JSONObject) = Field(
        value = if (o.isNull("value")) null else o.optString("value").ifEmpty { null },
        source = o.optString("source", "manual"),
        confidence = o.optDouble("confidence", 0.0),
        evidence = o.optJSONArray("evidence").strings(),
        alternates = o.optJSONArray("alternates").strings(),
    )

    private fun draft(o: JSONObject) = Draft(
        id = o.getString("id"),
        book = o.getLong("book"),
        receipt = field(o.getJSONObject("receipt")),
        name = field(o.getJSONObject("name")),
        amount = field(o.getJSONObject("amount")),
        mode = field(o.getJSONObject("mode")),
        status = o.optString("status", "pending"),
        rawText = o.optString("raw_text", ""),
        visionNote = o.optString("vision_note", ""),
        transcript = o.optString("transcript", ""),
        photoUrl = o.optString("photo_url", ""),
        audioUrl = if (o.isNull("audio_url")) null else o.optString("audio_url"),
        warnings = o.optJSONArray("warnings").strings(),
        createdAt = o.optString("created_at", ""),
        committedRow = if (o.isNull("committed_row")) null else o.optInt("committed_row"),
    )

    private fun totals(o: JSONObject) =
        Totals(o.optLong("cash"), o.optLong("online"), o.optLong("total"))

    // -------------------------------------------------------------- endpoints

    fun health(): Health {
        val o = JSONObject(get("/api/health"))
        return Health(
            version = o.optString("version", "?"),
            vision = o.optString("vision", "none"),
            stt = o.optString("stt", "none"),
            visionReady = o.optBoolean("vision_ready", false),
            sttReady = o.optBoolean("stt_ready", false),
            ledgerOk = o.optBoolean("ledger_ok", false),
            ledgerPath = o.optString("ledger_path", ""),
            activeBook = if (o.isNull("active_book")) null else o.getLong("active_book"),
        )
    }

    fun book(): BookInfo {
        val o = JSONObject(get("/api/book"))
        return BookInfo(
            book = if (o.isNull("book")) null else o.getLong("book"),
            recent = o.optJSONArray("recent_books").numbers(),
        )
    }

    fun setBook(book: Long) = JSONObject(post("/api/book", JSONObject().put("book", book)))

    fun drafts(limit: Int = 25) = JSONArray(get("/api/drafts?limit=$limit")).objects().map { draft(it) }

    fun draft(id: String) = draft(JSONObject(get("/api/draft/$id")))

    fun edit(id: String, body: JSONObject) = draft(JSONObject(patch("/api/draft/$id", body)))

    fun confirm(id: String): ConfirmResult {
        val o = JSONObject(post("/api/draft/$id/confirm"))
        return ConfirmResult(draft(o.getJSONObject("draft")), o.getInt("row"), totals(o.getJSONObject("totals")))
    }

    fun reject(id: String) = draft(JSONObject(post("/api/draft/$id/reject")))

    fun stats(): Stats {
        val o = JSONObject(get("/api/stats"))
        return Stats(
            todayTotal = o.optLong("today_total"),
            todayReceipts = o.optLong("today_receipts"),
            ledgerCash = o.optLong("ledger_cash"),
            ledgerOnline = o.optLong("ledger_online"),
            ledgerTotal = o.optLong("ledger_total"),
            ledgerRow = o.optLong("ledger_row"),
        )
    }

    /** One receipt: photo + the 6-second clip. The server does the reading. */
    fun capture(image: File, audio: File?, transcript: String, book: Long, clientId: String): Draft {
        val form = MultipartBody.Builder().setType(MultipartBody.FORM)
            .addFormDataPart("image", "receipt.jpg", image.asRequestBody("image/jpeg".toMediaType()))
            .addFormDataPart("transcript", transcript)
            .addFormDataPart("book", book.toString())
            .addFormDataPart("client_id", clientId)
        if (audio != null && audio.length() > 0) {
            form.addFormDataPart("audio", "voice.wav", audio.asRequestBody("audio/wav".toMediaType()))
        }
        val body = form.build()
        return draft(JSONObject(exec(req("/api/capture").post(body))))
    }

    /** Photos and clips are served behind the same PIN, carried in the query. */
    fun mediaUrl(path: String): String {
        if (path.isEmpty()) return ""
        val sep = if (path.contains("?")) "&" else "?"
        val pin = if (store.pin.isEmpty()) "" else "$sep" + "pin=" + URLEncoder.encode(store.pin, "UTF-8")
        return store.base + path + pin
    }

    /** Blocking; call from a background dispatcher. A stub photo is a few MB. */
    fun photo(path: String): Bitmap? = try {
        http.newCall(req(path).get().build()).execute().use { res ->
            if (res.isSuccessful) BitmapFactory.decodeStream(res.body?.byteStream()) else null
        }
    } catch (_: Exception) {
        null
    }
}

private fun JSONArray?.strings(): List<String> {
    if (this == null) return emptyList()
    return (0 until length()).mapNotNull { if (isNull(it)) null else optString(it).ifEmpty { null } }
}

private fun JSONArray?.numbers(): List<Long> {
    if (this == null) return emptyList()
    return (0 until length()).map { optLong(it) }
}

private fun JSONArray?.objects(): List<JSONObject> {
    if (this == null) return emptyList()
    return (0 until length()).mapNotNull { optJSONObject(it) }
}
