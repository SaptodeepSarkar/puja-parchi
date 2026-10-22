package org.pujaparchi.app

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * The reviewer's screen: what the server read, where each value came from, and
 * one button that writes the row. Manual corrections are sent only for the
 * fields actually touched, so the evidence of the untouched ones survives.
 */
@Composable
fun ReviewScreen(
    api: Api,
    draftId: String,
    onDone: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var draft by remember { mutableStateOf<Draft?>(null) }
    var photo by remember { mutableStateOf<Bitmap?>(null) }
    var receipt by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var amount by remember { mutableStateOf("") }
    var online by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var pushed by remember { mutableStateOf<ConfirmResult?>(null) }

    LaunchedEffect(draftId) {
        try {
            val d = withContext(Dispatchers.IO) { api.draft(draftId) }
            draft = d
            receipt = d.receipt.value.orEmpty()
            name = d.name.value.orEmpty()
            amount = d.amount.value.orEmpty()
            online = d.mode.value?.uppercase() == "ONLINE"
            photo = withContext(Dispatchers.IO) { api.photo(d.photoUrl) }
        } catch (e: Exception) {
            error = e.message
        }
    }

    val d = draft
    if (d == null) {
        Column(Modifier.padding(16.dp)) {
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) } ?: Text("Loading…")
        }
        return
    }

    val done = pushed
    if (done != null) {
        Column(
            Modifier
                .fillMaxSize()
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Pushed", fontSize = 26.sp, fontWeight = FontWeight.Bold, color = Color(0xFF15803D))
            Text("Row ${done.row} is in the book.", fontSize = 18.sp)
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFFF1F7F1))) {
                Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text("${done.draft.name.value.orEmpty()} · R/${done.draft.receipt.value.orEmpty()}")
                    Text(
                        "${done.draft.mode.value.orEmpty()} ₹${done.draft.amount.value.orEmpty()}",
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text("Cash ₹${done.totals.cash} · Online ₹${done.totals.online}")
                    Text("Book total ₹${done.totals.total}", fontWeight = FontWeight.Bold)
                }
            }
            Button(
                onClick = onDone,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(52.dp),
            ) { Text("Next receipt", fontSize = 17.sp) }
        }
        return
    }

    Column(
        Modifier
            .fillMaxSize()
            .padding(16.dp)
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Book ${d.book}", fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.width(10.dp))
            Text("Check all four", fontWeight = FontWeight.Bold, fontSize = 17.sp)
        }

        error?.let {
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFFFDECEC))) {
                Text(it, Modifier.padding(12.dp), color = Color(0xFF8A1C1C))
            }
        }

        photo?.let {
            Image(
                it.asImageBitmap(),
                contentDescription = "the stub",
                modifier = Modifier
                    .fillMaxWidth()
                    .height(180.dp)
                    .clip(RoundedCornerShape(12.dp)),
            )
        }

        if (d.transcript.isNotBlank()) {
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFFFFF7EC))) {
                Column(Modifier.padding(12.dp)) {
                    Text("You said", fontWeight = FontWeight.SemiBold)
                    Text(d.transcript)
                }
            }
        } else {
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFFFFF6E5))) {
                Text(
                    "No words were heard on the clip. Type the four values below.",
                    Modifier.padding(12.dp),
                )
            }
        }

        ValueField("Receipt number", receipt, d.receipt) { receipt = it }
        ValueField("Name", name, d.name) { name = it }
        ValueField("Amount (rupees)", amount, d.amount) { amount = it }

        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Payment", fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.width(12.dp))
            FilterChip(selected = !online, onClick = { online = false }, label = { Text("Cash") })
            Spacer(Modifier.width(8.dp))
            FilterChip(selected = online, onClick = { online = true }, label = { Text("Online") })
        }

        if (d.rawText.isNotBlank()) {
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFFF7F2EA))) {
                Column(Modifier.padding(12.dp)) {
                    Text("Read off the stub", fontWeight = FontWeight.SemiBold)
                    Text(d.rawText, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }

        if (d.warnings.isNotEmpty()) {
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFFFFF6E5))) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text("Look twice", fontWeight = FontWeight.SemiBold, color = Color(0xFF7A4A12))
                    d.warnings.forEach { Text("• $it", color = Color(0xFF7A4A12)) }
                }
            }
        }

        // Only what the reviewer actually changed is sent, so the server keeps
        // the vision/voice evidence on the fields left alone.
        val changed = JSONObject().apply {
            if (receipt.trim() != d.receipt.value.orEmpty()) put("receipt", receipt.trim())
            if (name.trim() != d.name.value.orEmpty()) put("name", name.trim())
            if (amount.trim() != d.amount.value.orEmpty()) put("amount", amount.trim())
            val mode = if (online) "online" else "cash"
            if (mode != d.mode.value.orEmpty().lowercase()) put("mode", mode)
        }

        Button(
            onClick = {
                scope.launch {
                    busy = true
                    error = null
                    try {
                        if (changed.length() > 0) {
                            val edited = withContext(Dispatchers.IO) { api.edit(d.id, changed) }
                            draft = edited
                        }
                        pushed = withContext(Dispatchers.IO) { api.confirm(d.id) }
                    } catch (e: Exception) {
                        error = e.message
                        // A 409 means the row already exists; re-read it so the
                        // screen stops showing it as pushable.
                        if (e is ApiException && e.status == 409) {
                            runCatching { draft = withContext(Dispatchers.IO) { api.draft(d.id) } }
                        }
                    } finally {
                        busy = false
                    }
                }
            },
            enabled = !busy && name.isNotBlank() && receipt.isNotBlank() && amount.isNotBlank(),
            modifier = Modifier
                .fillMaxWidth()
                .height(54.dp),
        ) { Text(if (busy) "Pushing…" else "Push to the ledger", fontSize = 17.sp) }

        OutlinedButton(
            onClick = {
                scope.launch {
                    runCatching { withContext(Dispatchers.IO) { api.reject(d.id) } }
                    onDone()
                }
            },
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Throw this capture away") }
    }
}

@Composable
private fun ValueField(label: String, value: String, field: Field, onChange: (String) -> Unit) {
    Column {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(label, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.width(8.dp))
            SourceBadge(field)
        }
        OutlinedTextField(
            value = value,
            onValueChange = onChange,
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        field.evidence.firstOrNull()?.let {
            Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 12.sp)
        }
    }
}

/** Where a value came from, which is the whole point of the review screen. */
@Composable
private fun SourceBadge(field: Field) {
    val (label, colour) = when (field.source) {
        "voice" -> "heard" to Color(0xFF1D4ED8)
        "vision" -> "read" to Color(0xFF6D28D9)
        "namedb" -> "spelling" to Color(0xFF0F766E)
        "manual" -> "typed" to Color(0xFF6B7280)
        "default" -> "assumed" to Color(0xFF92400E)
        else -> field.source to Color(0xFF6B7280)
    }
    val percent = (field.confidence * 100).toInt()
    Box(
        Modifier
            .clip(RoundedCornerShape(50))
            .background(colour.copy(alpha = 0.12f))
            .padding(horizontal = 8.dp, vertical = 2.dp)
    ) {
        Text(
            if (percent > 0) "$label $percent%" else label,
            color = colour,
            fontSize = 12.sp,
            fontWeight = FontWeight.Medium,
        )
    }
}
