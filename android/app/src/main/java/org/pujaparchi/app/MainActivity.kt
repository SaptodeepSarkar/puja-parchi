package org.pujaparchi.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private enum class Tab { Capture, Recent, Setup }

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme(
                colorScheme = lightColorScheme(
                    primary = Color(0xFF7F1D1D),
                    onPrimary = Color.White,
                    secondary = Color(0xFFD97706),
                    background = Color(0xFFF7F2EA),
                    surface = Color.White,
                ),
            ) { PujaApp() }
        }
    }
}

@Composable
private fun PujaApp() {
    val ctx = LocalContext.current
    val store = remember { Store(ctx) }
    val api = remember { Api(store) }
    val scope = rememberCoroutineScope()

    var tab by remember { mutableStateOf(if (store.base.isEmpty()) Tab.Setup else Tab.Capture) }
    var book by remember { mutableStateOf<Long?>(null) }
    var reviewId by remember { mutableStateOf<String?>(null) }
    var offline by remember { mutableStateOf<String?>(null) }

    // The book lives on the server so the phone and the monitor never disagree.
    fun refreshBook() {
        if (store.base.isEmpty()) return
        scope.launch {
            try {
                book = withContext(Dispatchers.IO) { api.book() }.book
                offline = null
            } catch (e: Exception) {
                offline = e.message
            }
        }
    }

    LaunchedEffect(tab) { refreshBook() }

    Scaffold(
        bottomBar = {
            if (reviewId == null) {
                NavigationBar {
                    NavigationBarItem(
                        selected = tab == Tab.Capture,
                        onClick = { tab = Tab.Capture },
                        icon = {},
                        label = { Text("Capture") },
                    )
                    NavigationBarItem(
                        selected = tab == Tab.Recent,
                        onClick = { tab = Tab.Recent },
                        icon = {},
                        label = { Text("Recent") },
                    )
                    NavigationBarItem(
                        selected = tab == Tab.Setup,
                        onClick = { tab = Tab.Setup },
                        icon = {},
                        label = { Text("Setup") },
                    )
                }
            }
        },
    ) { pad ->
        Column(
            Modifier
                .padding(pad)
                .fillMaxSize(),
        ) {
            offline?.let {
                Text(
                    "Server: $it",
                    Modifier
                        .fillMaxWidth()
                        .padding(8.dp),
                    color = MaterialTheme.colorScheme.error,
                    fontSize = 12.sp,
                )
            }
            val id = reviewId
            when {
                id != null -> ReviewScreen(api, id, onDone = {
                    reviewId = null
                    refreshBook()
                })

                tab == Tab.Capture -> CaptureScreen(
                    api = api,
                    book = book,
                    onDraft = { reviewId = it.id },
                    onSetup = { tab = Tab.Setup },
                )

                tab == Tab.Recent -> RecentScreen(api, onOpen = { reviewId = it })

                else -> SetupScreen(api, store, onSaved = { tab = Tab.Capture; refreshBook() })
            }
        }
    }
}

@Composable
private fun SetupScreen(api: Api, store: Store, onSaved: () -> Unit) {
    val scope = rememberCoroutineScope()
    var base by remember { mutableStateOf(store.base) }
    var pin by remember { mutableStateOf(store.pin) }
    var bookText by remember { mutableStateOf("") }
    var health by remember { mutableStateOf<Health?>(null) }
    var recent by remember { mutableStateOf<List<Long>>(emptyList()) }
    var note by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }

    Column(
        Modifier
            .fillMaxSize()
            .padding(16.dp)
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Setup", fontSize = 22.sp, fontWeight = FontWeight.Bold)
        Text(
            "The laptop that runs the server must be on the same wifi. Its address is printed " +
                "when the server starts, or run `hostname -I` on the laptop.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        OutlinedTextField(
            value = base,
            onValueChange = { base = it },
            label = { Text("Server address") },
            placeholder = { Text("http://192.168.0.110:8080") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = pin,
            onValueChange = { pin = it },
            label = { Text("PIN (only if the server asks for one)") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )

        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Button(
                enabled = !busy,
                onClick = {
                    store.base = base
                    store.pin = pin
                    scope.launch {
                        busy = true
                        note = ""
                        try {
                            val h = withContext(Dispatchers.IO) { api.health() }
                            health = h
                            recent = withContext(Dispatchers.IO) { api.book() }.recent
                            note = "Connected to version ${h.version}."
                        } catch (e: Exception) {
                            health = null
                            note = e.message ?: "Could not reach the server."
                        } finally {
                            busy = false
                        }
                    }
                },
            ) { Text("Save and test") }
        }

        health?.let { h ->
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFFF1F7F1))) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("Vision model", fontWeight = FontWeight.SemiBold)
                        Spacer(Modifier.width(8.dp))
                        Text(
                            if (h.vision == "none") "not configured" else h.vision,
                            color = if (h.vision == "none") Color(0xFF92400E) else Color(0xFF15803D),
                        )
                    }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("Speech model", fontWeight = FontWeight.SemiBold)
                        Spacer(Modifier.width(8.dp))
                        Text(
                            if (h.stt == "none") "not configured" else h.stt,
                            color = if (h.stt == "none") Color(0xFF92400E) else Color(0xFF15803D),
                        )
                    }
                    Text(if (h.ledgerOk) "Ledger: found" else "Ledger: MISSING")
                    Text(h.ledgerPath, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    h.activeBook?.let { Text("Receipt book: $it", fontWeight = FontWeight.SemiBold) }
                }
            }
        }

        note.takeIf { it.isNotEmpty() }?.let {
            Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }

        HorizontalDivider()

        Text("Receipt book number", fontSize = 18.sp, fontWeight = FontWeight.Bold)
        Text(
            "Chosen here, fixed until you change it, and never read off the photo.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            OutlinedTextField(
                value = bookText,
                onValueChange = { bookText = it.filter { c -> c.isDigit() } },
                label = { Text("Book") },
                singleLine = true,
                modifier = Modifier.width(140.dp),
            )
            Button(
                enabled = bookText.isNotEmpty() && !busy,
                onClick = {
                    scope.launch {
                        busy = true
                        try {
                            withContext(Dispatchers.IO) { api.setBook(bookText.toLong()) }
                            note = "Book ${bookText.toLong()} is now the active book."
                            recent = withContext(Dispatchers.IO) { api.book() }.recent
                            onSaved()
                        } catch (e: Exception) {
                            note = e.message ?: "Could not set the book."
                        } finally {
                            busy = false
                        }
                    }
                },
            ) { Text("Set book") }
        }

        if (recent.isNotEmpty()) {
            Text("Used before", fontWeight = FontWeight.SemiBold)
            recent.forEach { b ->
                Text(
                    "Book $b",
                    Modifier
                        .fillMaxWidth()
                        .clickable { bookText = b.toString() }
                        .padding(vertical = 6.dp),
                )
            }
        }

        HorizontalDivider()
        Text(
            "Nothing is written to Excel until you tap Push on the review screen. " +
                "The photo and the clip are saved before any model sees them.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun RecentScreen(api: Api, onOpen: (String) -> Unit) {
    var drafts by remember { mutableStateOf<List<Draft>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    fun load() {
        scope.launch {
            busy = true
            try {
                drafts = withContext(Dispatchers.IO) { api.drafts(40) }
                error = null
            } catch (e: Exception) {
                error = e.message
            } finally {
                busy = false
            }
        }
    }

    LaunchedEffect(Unit) { load() }

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Recent captures", fontSize = 20.sp, fontWeight = FontWeight.Bold)
            Spacer(Modifier.width(12.dp))
            OutlinedButton(onClick = { load() }, enabled = !busy) { Text("Refresh") }
        }
        error?.let { Text(it, Modifier.padding(horizontal = 16.dp), color = MaterialTheme.colorScheme.error) }

        val list = drafts
        if (list == null) {
            Text("Loading…", Modifier.padding(16.dp))
        } else if (list.isEmpty()) {
            Text("Nothing captured yet.", Modifier.padding(16.dp))
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(list, key = { it.id }) { d ->
                    Card(
                        Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp, vertical = 5.dp)
                            .clickable(enabled = d.status == "pending") { onOpen(d.id) },
                        colors = CardDefaults.cardColors(
                            containerColor = if (d.status == "pending") Color.White else Color(0xFFF1EEE9),
                        ),
                    ) {
                        Column(Modifier.padding(12.dp)) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(
                                    d.name.value ?: "(no name)",
                                    fontWeight = FontWeight.Bold,
                                    fontSize = 16.sp,
                                )
                                Spacer(Modifier.width(8.dp))
                                StatusChip(d)
                            }
                            Text(
                                "R/${d.receipt.value ?: "?"} · ${d.mode.value ?: "?"} · ₹${d.amount.value ?: "?"} · book ${d.book}",
                            )
                            Text(
                                when (d.status) {
                                    "committed" -> "row ${d.committedRow ?: "?"}"
                                    else -> "tap to review"
                                },
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                fontSize = 12.sp,
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun StatusChip(d: Draft) {
    val (text, colour) = when (d.status) {
        "committed" -> "pushed" to Color(0xFF15803D)
        "rejected" -> "thrown away" to Color(0xFF6B7280)
        else -> "waiting" to Color(0xFF92400E)
    }
    Text(
        text,
        Modifier.padding(horizontal = 8.dp, vertical = 2.dp),
        color = colour,
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
    )
}
