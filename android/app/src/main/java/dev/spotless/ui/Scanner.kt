package dev.spotless.ui

import android.Manifest
import android.content.pm.PackageManager
import android.util.Size
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size as Area
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.codeInFrame
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The camera, looking for a Spotify Code. Each frame is read on the phone;
 * once two in a row read the same code, Spotify is asked what it is (once)
 * and Browse opens it. Back or CLOSE leaves. The camera permission is asked
 * for the first time.
 */
@Composable
fun Scanner(core: Core) {
    val context = LocalContext.current
    var allowed by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED,
        )
    }
    var refused by remember { mutableStateOf(false) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        allowed = granted
        refused = !granted
    }
    LaunchedEffect(Unit) { if (!allowed) ask.launch(Manifest.permission.CAMERA) }
    Column(Modifier.fillMaxSize().background(Color.Black).safeDrawingPadding()) {
        Box(Modifier.weight(1f).fillMaxWidth()) {
            when {
                allowed -> {
                    Camera(core)
                    Guide()
                }
                refused -> BasicText(
                    "Spotless can't use the camera. Allow it in Android's settings for Spotless, " +
                        "or pick a screenshot of the code instead.",
                    Modifier.align(Alignment.Center).padding(24.dp),
                    body(color = Classic.dim),
                )
            }
        }
        Row(
            Modifier.fillMaxWidth().background(Classic.frame).padding(8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            BasicText(
                "Hold a Spotify Code in the frame, filling it",
                Modifier.weight(1f).padding(start = 4.dp),
                body(14.sp, Classic.label),
            )
            TextButton("CLOSE", { core.scanning = false }, Modifier.height(48.dp))
        }
    }
}

/** Where to hold the code: a wide frame in the middle, as a code is shaped. */
@Composable
private fun Guide() {
    Canvas(Modifier.fillMaxSize()) {
        val width = size.width * 0.86f
        val height = width / 4f
        val corner = Offset((size.width - width) / 2, (size.height - height) / 2)
        drawRoundRect(
            Classic.lcd,
            corner,
            Area(width, height),
            CornerRadius(12.dp.toPx()),
            style = Stroke(2.dp.toPx()),
        )
    }
}

/** The camera's preview, with its frames read for a code. */
@Composable
private fun Camera(core: Core) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val view = remember { PreviewView(context).apply { scaleType = PreviewView.ScaleType.FILL_CENTER } }
    DisposableEffect(owner) {
        val reading = Executors.newSingleThreadExecutor()
        val found = AtomicBoolean(false)
        // The code the last frame read: the next must read the same.
        var last: ULong? = null
        val analysis = ImageAnalysis.Builder()
            // About VGA: enough for a code that fills the frame, and quick.
            .setResolutionSelector(
                ResolutionSelector.Builder()
                    .setResolutionStrategy(
                        ResolutionStrategy(Size(640, 480), ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER),
                    )
                    .build(),
            )
            .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
            .build()
        analysis.setAnalyzer(reading) { frame ->
            val code = frame.use { codeInFrame(it.width.toUInt(), it.height.toUInt(), greys(it)) }
            val again = code != null && code == last
            last = code
            if (again && found.compareAndSet(false, true)) {
                ContextCompat.getMainExecutor(context).execute {
                    core.scanning = false
                    core.apply(Action.LookUpCode(code!!))
                }
            }
        }
        val providing = ProcessCameraProvider.getInstance(context)
        providing.addListener({
            val provider = providing.get()
            val preview = Preview.Builder().build().also { it.surfaceProvider = view.surfaceProvider }
            provider.unbindAll()
            provider.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
        }, ContextCompat.getMainExecutor(context))
        onDispose {
            if (providing.isDone) providing.get().unbindAll()
            reading.shutdown()
        }
    }
    AndroidView({ view }, Modifier.fillMaxSize())
}

/** A frame's brightness (its Y plane), a byte a pixel, row by row without padding. */
private fun greys(frame: ImageProxy): ByteArray {
    val plane = frame.planes[0]
    val buffer = plane.buffer.duplicate().apply { rewind() }
    val (width, height) = frame.width to frame.height
    val stride = plane.rowStride
    val greys = ByteArray(width * height)
    if (stride == width && plane.pixelStride == 1) {
        buffer.get(greys, 0, width * height)
    } else {
        for (y in 0 until height) {
            buffer.position(y * stride)
            if (plane.pixelStride == 1) {
                buffer.get(greys, y * width, width)
            } else {
                for (x in 0 until width) greys[y * width + x] = buffer.get(y * stride + x * plane.pixelStride)
            }
        }
    }
    return greys
}
