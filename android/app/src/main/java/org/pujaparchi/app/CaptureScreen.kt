package org.pujaparchi.app

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageCapture
import androidx.camera.core.ImageCaptureException
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.util.UUID

private enum class Stage { Aim, Ready, Recording, Sending }

/**
 * Photo, then six seconds of speech, then straight to review.
 *
 * The ordered script is on screen *before* the clock starts, because a
 * volunteer who has not been told what to say says one word and stops.
 */
@Composable
fun CaptureScreen(
    api: Api,
    book: Long?,
    onDraft: (Draft) -> Unit,
    onSetup: () -> Unit,
) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val recorder = remember { VoiceRecorder(ctx) }

    var stage by remember { mutableStateOf(Stage.Aim) }
    var photo by remember { mutableStateOf<File?>(null) }
    var thumb by remember { mutableStateOf<Bitmap?>(null) }
    var clientId by remember { mutableStateOf("") }
    var left by remember { mutableIntStateOf(SECONDS) }
    var error by remember { mutableStateOf<String?>(null) }
    var status by remember { mutableStateOf("") }

    var allowed by remember { mutableStateOf(hasCamera(ctx) && hasMic(ctx)) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        allowed = hasCamera(ctx) && hasMic(ctx)
    }
    LaunchedEffect(Unit) {
        if (!allowed) ask.launch(arrayOf(Manifest.permission.CAMERA, Manifest.permission.RECORD_AUDIO))
    }

    val send: () -> Unit = {
        scope.launch {
            val clip = withContext(Dispatchers.IO) { recorder.stop() }
            val picture = photo
            if (picture == null) {
                error = "No photo to send."
                stage = Stage.Ready
            } else if (book == null) {
                error = "Set the receipt book number first."
                stage = Stage.Ready
            } else {
                stage = Stage.Sending
                status = "Reading the stub and listening to the clip…"
                try {
                    val draft = withContext(Dispatchers.IO) {
                        api.capture(picture, clip, "", book, clientId)
                    }
                    status = ""
                    onDraft(draft)
                } catch (e: Exception) {
                    error = e.message ?: "The server refused the capture."
                    status = ""
                    stage = Stage.Ready
                }
            }
        }
    }

    // The clock. Runs once per recording, and only the clock ends a recording.
    LaunchedEffect(stage) {
        if (stage == Stage.Recording) {
            try {
                withContext(Dispatchers.IO) { recorder.start() }
            } catch (e: Exception) {
                error = e.message
                stage = Stage.Ready
                return@LaunchedEffect
            }
            left = SECONDS
            while (left > 0) {
                delay(1000)
                left -= 1
            }
            send()
        }
    }

    Column(
        Modifier
            .fillMaxSize()
            .padding(16.dp)
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Book", fontWeight = FontWeight.SemiBold)
            Text(
                if (book == null) " not set" else " $book",
                fontWeight = FontWeight.Bold,
                color = if (book == null) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary,
            )
            Spacer(Modifier.width(8.dp))
            OutlinedButton(onClick = onSetup) { Text("Change") }
        }

        error?.let {
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFFFDECEC))) {
                Text(
                    it,
                    Modifier.padding(12.dp),
                    color = Color(0xFF8A1C1C),
                    fontWeight = FontWeight.Medium,
                )
            }
        }

        if (!allowed) {
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFFFFF6E5))) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("The camera and the microphone are both needed.", fontWeight = FontWeight.SemiBold)
                    Text("The photo is how the receipt gets in; the 6-second clip is how the numbers get in.")
                    Button(onClick = {
                        ask.launch(arrayOf(Manifest.permission.CAMERA, Manifest.permission.RECORD_AUDIO))
                    }) { Text("Allow camera and mic") }
                }
            }
        }

        when (stage) {
            Stage.Aim -> {
                Text("Photograph the stub", fontWeight = FontWeight.SemiBold)
                Text(
                    "Lay the stub flat, fill the frame. This photo is what the server reads.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                CameraBox(
                    enabled = allowed && book != null,
                    onPicture = { file ->
                        photo = file
                        clientId = UUID.randomUUID().toString()
                        thumb = null
                        error = null
                        scope.launch {
                            thumb = withContext(Dispatchers.IO) { downscale(file) }
                        }
                        stage = Stage.Ready
                    },
                    onError = { error = it },
                )
                if (book == null) {
                    Text(
                        "Choose the receipt book number before the first capture — it is never read off the photo.",
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }

            Stage.Ready, Stage.Recording, Stage.Sending -> {
                thumb?.let {
                    Image(
                        it.asImageBitmap(),
                        contentDescription = "the stub you just photographed",
                        modifier = Modifier
                            .height(120.dp)
                            .clip(RoundedCornerShape(10.dp)),
                    )
                }

                OrderedScript()

                when (stage) {
                    Stage.Ready -> {
                        Button(
                            onClick = { error = null; stage = Stage.Recording },
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(52.dp),
                        ) { Text("Start the 6 seconds", fontSize = 17.sp) }
                        Text(
                            "You get six seconds to say all four. Tap stop when you finish early.",
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }

                    Stage.Recording -> {
                        Text("Say them now — $left", fontSize = 20.sp, fontWeight = FontWeight.Bold)
                        LinearProgressIndicator(
                            progress = { (SECONDS - left).toFloat() / SECONDS },
                            modifier = Modifier.fillMaxWidth(),
                        )
                        OutlinedButton(
                            onClick = { left = 0 },
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(52.dp),
                        ) { Text("Stop — I have said everything") }
                        Text(
                            "It is listening. Do not hang up the words: name, receipt, mode, amount.",
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }

                    else -> {
                        Text("Working…", fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
                        LinearProgressIndicator(Modifier.fillMaxWidth())
                        Text(status, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }

                if (stage == Stage.Ready) {
                    OutlinedButton(onClick = {
                        photo = null
                        thumb = null
                        error = null
                        stage = Stage.Aim
                    }) { Text("Retake the photo") }
                }
            }
        }
    }
}

/**
 * The part that was missing: what to say, in which order, with an example.
 * Nobody is born knowing this.
 */
@Composable
private fun OrderedScript() {
    Card(colors = CardDefaults.cardColors(containerColor = Color(0xFFFFF7EC))) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Say it in this order", fontWeight = FontWeight.Bold, fontSize = 17.sp)
            ScriptLine("1", "Name", "Sudha Dairy")
            ScriptLine("2", "Receipt number", "nine five zero one")
            ScriptLine("3", "Cash or online", "cash")
            ScriptLine("4", "Amount", "eleven hundred")
            Text(
                "Four things, one after another, in one breath. Then stop.",
                color = Color(0xFF7A4A12),
                fontWeight = FontWeight.Medium,
            )
        }
    }
}

@Composable
private fun ScriptLine(n: String, what: String, said: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Box(
            Modifier
                .size(24.dp)
                .clip(RoundedCornerShape(50))
                .background(Color(0xFF7F1D1D)),
            contentAlignment = Alignment.Center,
        ) { Text(n, color = Color.White, fontWeight = FontWeight.Bold, fontSize = 13.sp) }
        Spacer(Modifier.width(10.dp))
        Column {
            Text(what, fontWeight = FontWeight.SemiBold)
            Text(
                "say: \"$said\"",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                fontFamily = FontFamily.Monospace,
                fontSize = 13.sp,
            )
        }
    }
}

@Composable
private fun CameraBox(enabled: Boolean, onPicture: (File) -> Unit, onError: (String) -> Unit) {
    val ctx = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val preview = remember { PreviewView(ctx) }
    var capture by remember { mutableStateOf<ImageCapture?>(null) }

    LaunchedEffect(enabled) {
        if (!enabled) return@LaunchedEffect
        val future = ProcessCameraProvider.getInstance(ctx)
        future.addListener({
            val provider = future.get()
            val view = Preview.Builder().build()
            view.setSurfaceProvider(preview.surfaceProvider)
            val cap = ImageCapture.Builder().setCaptureMode(ImageCapture.CAPTURE_MODE_MINIMIZE_LATENCY).build()
            capture = cap
            provider.unbindAll()
            provider.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, view, cap)
        }, ContextCompat.getMainExecutor(ctx))
    }

    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(
            Modifier
                .fillMaxWidth()
                .height(300.dp)
                .clip(RoundedCornerShape(14.dp))
                .background(Color.Black),
        ) { AndroidView({ preview }, Modifier.fillMaxSize()) }

        Button(
            onClick = {
                val target = File(ctx.cacheDir, "stub-${System.currentTimeMillis()}.jpg")
                val shot = capture ?: return@Button onError("The camera is still starting. One more second.")
                shot.takePicture(
                    ImageCapture.OutputFileOptions.Builder(target).build(),
                    ContextCompat.getMainExecutor(ctx),
                    object : ImageCapture.OnImageSavedCallback {
                        override fun onImageSaved(results: ImageCapture.OutputFileResults) = onPicture(target)
                        override fun onError(e: ImageCaptureException) =
                            onError("The camera did not save that photo: ${e.message}")
                    },
                )
            },
            enabled = enabled,
            modifier = Modifier
                .fillMaxWidth()
                .height(56.dp),
            colors = ButtonDefaults.buttonColors(),
        ) { Text("Photograph the stub", fontSize = 17.sp) }
    }
}

private const val SECONDS = 6

private fun hasCamera(ctx: Context) =
    ContextCompat.checkSelfPermission(ctx, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED

private fun hasMic(ctx: Context) =
    ContextCompat.checkSelfPermission(ctx, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

/** Keeps a 12 MP stub from being decoded at full size just to draw 120 dp. */
private fun downscale(file: File): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(file.absolutePath, bounds)
    if (bounds.outWidth <= 0) return null
    var sample = 1
    while (bounds.outWidth / (sample * 2) >= 640) sample *= 2
    return BitmapFactory.decodeFile(file.absolutePath, BitmapFactory.Options().apply { inSampleSize = sample })
}
