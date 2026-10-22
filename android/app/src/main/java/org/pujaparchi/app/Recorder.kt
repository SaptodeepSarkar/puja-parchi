package org.pujaparchi.app

import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import java.io.File
import java.io.FileOutputStream
import java.io.RandomAccessFile
import kotlin.concurrent.thread

/**
 * Six seconds of speech, written as a 16 kHz mono WAV.
 *
 * WAV rather than AAC or webm because every speech model takes it without a
 * converter. There is deliberately no on-device dictation: two live microphone
 * users is how the browser version silently lost the words, so here the phone
 * only records and the server transcribes.
 */
class VoiceRecorder(private val ctx: Context) {

    private val rate = 16_000
    private val maxSeconds = 10 // hard stop, in case the UI ever fails to

    private var worker: Thread? = null

    @Volatile
    private var running = false

    private var out: File? = null

    @SuppressLint("MissingPermission") // the capture screen holds the permission
    fun start(): File {
        val min = AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val buf = maxOf(min, rate)
        val rec = AudioRecord(
            MediaRecorder.AudioSource.VOICE_RECOGNITION,
            rate,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
            buf,
        )
        if (rec.state != AudioRecord.STATE_INITIALIZED) throw IllegalStateException("the microphone is busy")

        val file = File(ctx.cacheDir, "voice-${System.currentTimeMillis()}.wav")
        out = file
        running = true
        worker = thread(name = "voice") {
            FileOutputStream(file).use { sink ->
                sink.write(ByteArray(44)) // header, patched once we know the size
                val chunk = ByteArray(buf)
                var bytes = 0
                rec.startRecording()
                while (running && bytes < rate * 2 * maxSeconds) {
                    val n = rec.read(chunk, 0, chunk.size)
                    if (n > 0) {
                        sink.write(chunk, 0, n)
                        bytes += n
                    }
                }
                runCatching { rec.stop() }
                rec.release()
                sink.flush()
                sink.close()
                patchHeader(file, bytes)
            }
        }
        return file
    }

    /** Returns the clip, or null when nothing usable was recorded. */
    fun stop(): File? {
        running = false
        worker?.join(2000)
        worker = null
        val file = out
        out = null
        return file?.takeIf { it.length() > 44 + rate }
    }

    /** A 44-byte PCM WAV header. */
    private fun patchHeader(file: File, dataBytes: Int) {
        val h = ByteArray(44)
        fun put(offset: Int, text: String) = text.toByteArray(Charsets.US_ASCII).copyInto(h, offset)
        fun put32(offset: Int, v: Int) {
            h[offset] = (v and 0xff).toByte()
            h[offset + 1] = ((v ushr 8) and 0xff).toByte()
            h[offset + 2] = ((v ushr 16) and 0xff).toByte()
            h[offset + 3] = ((v ushr 24) and 0xff).toByte()
        }
        put(0, "RIFF")
        put32(4, 36 + dataBytes)
        put(8, "WAVE")
        put(12, "fmt ")
        put32(16, 16)          // PCM chunk size
        h[20] = 1.toByte()     // PCM
        h[22] = 1.toByte()     // mono
        put32(24, rate)
        put32(28, rate * 2)    // byte rate
        h[32] = 2.toByte()     // block align
        h[34] = 16.toByte()    // bits
        put(36, "data")
        put32(40, dataBytes)
        RandomAccessFile(file, "rw").use { it.seek(0); it.write(h) }
    }
}
