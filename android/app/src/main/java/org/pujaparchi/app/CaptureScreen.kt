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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.util.UUID

private enum class Stage { Aim, Sending }

/**
 * The whole capture: photograph the stub, and the server reads it.
 *
 * Nothing is spoken. The written stub has the name, the receipt number and the
 * amount on it already, so the vision model reads the paper and the review
 * screen is where a human fixes whatever handwriting cost.
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

    var stage by remember { mutableStateOf(Stage.Aim) }
    var thumb by remember { mutableStateOf<Bitmap?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var status by remember { mutableStateOf("") }

    var allowed by remember { mutableStateOf(hasCamera(ctx)) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        allowed = hasCamera(ctx)
    }
    LaunchedEffect(Unit) { if (!allowed) ask.launch(Manifest.permission.CAMERA) }

    val send: (File) -> Unit = { picture ->
        if (book == null) {
            error = "Set the receipt book number first."
        } else {
            scope.launch {
                error = null
                stage = Stage.Sending
                status = "Reading the stub…"
                try {
                    val draft = withContext(Dispatchers.IO) {
                        api.capture(picture, book, UUID.randomUUID().toString())
                    }
                    status = ""
                    stage = Stage.Aim
                    thumb = null
                    onDraft(draft)
                } catch (e: Exception) {
                    error = e.message ?: "The server refused the capture."
                    status = ""
                    stage = Stage.Aim
                }
            }
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
                    Text("The camera is needed.", fontWeight = FontWeight.SemiBold)
                    Text("The photo of the stub is the whole capture — nothing is spoken.")
                    Button(onClick = { ask.launch(Manifest.permission.CAMERA) }) { Text("Allow the camera") }
                }
            }
        }

        if (stage == Stage.Aim) {
            Text("Photograph the stub", fontWeight = FontWeight.SemiBold)
            Text(
                "Lay the stub flat, fill the frame, keep your shadow off it. The whole stub is what the server reads.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            CameraBox(
                enabled = allowed && book != null,
                onPicture = { file ->
                    thumb = null
                    scope.launch { thumb = withContext(Dispatchers.IO) { downscale(file) } }
                    send(file)
                },
                onError = { error = it },
            )
            if (book == null) {
                Text(
                    "Choose the receipt book number before the first capture — it is never read off the photo.",
                    color = MaterialTheme.colorScheme.error,
                )
            }
        } else {
            thumb?.let {
                Image(
                    it.asImageBitmap(),
                    contentDescription = "the stub you just photographed",
                    modifier = Modifier
                        .height(180.dp)
                        .clip(RoundedCornerShape(10.dp)),
                )
            }
            Text("Working…", fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            LinearProgressIndicator(Modifier.fillMaxWidth())
            Text(status, color = MaterialTheme.colorScheme.onSurfaceVariant)
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

private fun hasCamera(ctx: Context) =
    ContextCompat.checkSelfPermission(ctx, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED

/** Keeps a 12 MP stub from being decoded at full size just to draw 180 dp. */
private fun downscale(file: File): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(file.absolutePath, bounds)
    if (bounds.outWidth <= 0) return null
    var sample = 1
    while (bounds.outWidth / (sample * 2) >= 640) sample *= 2
    return BitmapFactory.decodeFile(file.absolutePath, BitmapFactory.Options().apply { inSampleSize = sample })
}
