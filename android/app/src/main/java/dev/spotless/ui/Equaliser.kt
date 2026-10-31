package dev.spotless.ui

import android.os.SystemClock
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.drag
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Popup
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.Equaliser
import dev.spotless.core.equaliserPresets
import kotlin.math.abs
import kotlin.math.roundToInt

/** How far a band or the preamp goes either way, in dB. */
private const val RANGE = 12f

/** What the bands are called under their sliders, and in the readout. */
private val LABELS = listOf("60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K")
private val NAMES = listOf("60 HZ", "170 HZ", "310 HZ", "600 HZ", "1 KHZ", "3 KHZ", "6 KHZ", "12 KHZ", "14 KHZ", "16 KHZ")

private val PRESETS = equaliserPresets()

private val SLIDER = 118.dp

/** A slider's index for the preamp; the bands are 0 to 9. */
private const val PREAMP = -1

/** Two taps on a slider this close together put it back to 0. */
private const val DOUBLE_TAP_MS = 320L

/** While a slider is dragged, the sound follows it this often at most. */
private const val SEND_MS = 40L

private fun Equaliser.at(slider: Int) = if (slider == PREAMP) preamp else bands[slider]

/** Moving a slider turns the equaliser on, as on the desktop. */
private fun Equaliser.with(slider: Int, value: Float): Equaliser {
    val db = ((value * 10).roundToInt() / 10f).coerceIn(-RANGE, RANGE)
    return if (slider == PREAMP) copy(on = true, preamp = db)
    else copy(on = true, bands = bands.toMutableList().also { it[slider] = db })
}

private fun db(value: Float) = when {
    value > 0.05f -> "+%.1f DB".format(value)
    value < -0.05f -> "−%.1f DB".format(-value)
    else -> "0.0 DB"
}

private fun preset(bands: List<Float>) =
    PRESETS.firstOrNull { preset -> preset.bands.zip(bands).all { (a, b) -> abs(a - b) < 0.05f } }

/**
 * The equaliser above the display (EQ), as the desktop's: ON, Winamp's
 * presets and RESET, the preamp and the ten bands. The desktop's curve has
 * no room here; the readout beside the buttons names the band being dragged,
 * or the preset the bands match.
 */
@Composable
fun EqualiserPanel(core: Core) {
    val saved = core.settings.equaliser
    // The slider being dragged and where it is: the sound catches up every SEND_MS.
    var dragged by remember { mutableStateOf<Pair<Int, Float>?>(null) }
    val equaliser = dragged?.let { (slider, db) -> saved.with(slider, db) } ?: saved
    val send = remember { longArrayOf(0L) }
    fun set(slider: Int, db: Float, final: Boolean) {
        val now = SystemClock.uptimeMillis()
        if (final || now - send[0] >= SEND_MS) {
            send[0] = now
            core.apply(Action.SetEqualiser(core.settings.equaliser.with(slider, db)))
        }
    }
    Column(
        Modifier.padding(start = 8.dp, end = 8.dp, top = 10.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(Modifier.height(44.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            TextButton(
                "ON", { core.apply(Action.SetEqualiser(saved.copy(on = !saved.on))) },
                Modifier.fillMaxHeight(), on = saved.on,
            )
            Presets(core, saved)
            TextButton(
                "RESET",
                { core.apply(Action.SetEqualiser(saved.copy(preamp = 0f, bands = List(10) { 0f }))) },
                Modifier.fillMaxHeight(),
            )
            val readout = when {
                dragged != null -> dragged!!.let { (slider, value) ->
                    (if (slider == PREAMP) "PREAMP" else NAMES[slider]) + " " + db(value)
                }
                !saved.on -> "EQ OFF"
                else -> (preset(saved.bands)?.name?.uppercase() ?: "CUSTOM") +
                    if (abs(saved.preamp) >= 0.05f) " · PRE " + db(saved.preamp) else ""
            }
            Box(
                Modifier.weight(1f).fillMaxHeight().lcd().padding(horizontal = 10.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                Line(readout, mono(13.sp, if (saved.on || dragged != null) Classic.lcd else Classic.dim))
            }
        }
        Row(Modifier.fillMaxWidth().lcd().padding(start = 4.dp, end = 4.dp, top = 10.dp, bottom = 4.dp)) {
            val column = Modifier.weight(1f)
            Slider(PREAMP, "PRE", "Preamp", equaliser, column, { dragged = it }, ::set)
            Column(Modifier.width(22.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Column(Modifier.height(SLIDER), verticalArrangement = Arrangement.SpaceBetween) {
                    for (mark in listOf("+12", "0", "-12")) {
                        BasicText(mark, Modifier.fillMaxWidth(), mono(9.sp, Classic.label).copy(textAlign = TextAlign.Center))
                    }
                }
            }
            for (band in 0 until 10) {
                Slider(band, LABELS[band], NAMES[band], equaliser, column, { dragged = it }, ::set)
            }
        }
    }
}

/** PRESETS ▾, and Winamp's presets over the songs while it is pressed in. */
@Composable
private fun Presets(core: Core, saved: Equaliser) {
    var choosing by remember { mutableStateOf(false) }
    Box(Modifier.fillMaxHeight()) {
        TextButton("PRESETS ▾", { choosing = !choosing }, Modifier.fillMaxHeight(), on = choosing)
        if (choosing) {
            val chosen = preset(saved.bands)
            Popup(alignment = Alignment.BottomStart, onDismissRequest = { choosing = false }) {
                Column(
                    Modifier.width(240.dp).heightIn(max = 340.dp).background(Classic.frame).bevel()
                        .padding(4.dp).verticalScroll(rememberScrollState()),
                ) {
                    for (preset in PRESETS) {
                        TextButton(
                            preset.name,
                            {
                                // Presets set the bands only, and turn the equaliser on.
                                core.apply(Action.SetEqualiser(saved.copy(on = true, bands = preset.bands)))
                                choosing = false
                            },
                            Modifier.fillMaxWidth(),
                            on = preset == chosen,
                        )
                    }
                }
            }
        }
    }
}

/**
 * One slider: it fills from 0 dB up or down, jumps to where it is touched
 * and follows the finger; two taps put it back to 0.
 */
@Composable
private fun Slider(
    slider: Int,
    mark: String,
    name: String,
    equaliser: Equaliser,
    modifier: Modifier,
    onDrag: (Pair<Int, Float>?) -> Unit,
    set: (Int, Float, Boolean) -> Unit,
) {
    val value = equaliser.at(slider)
    val on = equaliser.on
    val dragTo by rememberUpdatedState(onDrag)
    val setTo by rememberUpdatedState(set)
    val lastTap = remember { longArrayOf(0L) }
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Box(
            Modifier
                .fillMaxWidth()
                .height(SLIDER)
                .semantics {
                    contentDescription = name
                    stateDescription = db(value)
                    progressBarRangeInfo = ProgressBarRangeInfo(value, -RANGE..RANGE)
                    setProgress { setTo(slider, it, true); true }
                }
                .pointerInput(slider) {
                    awaitEachGesture {
                        val down = awaitFirstDown()
                        down.consume()
                        fun at(y: Float) = RANGE - y / size.height * 2 * RANGE
                        val now = down.uptimeMillis
                        if (now - lastTap[0] < DOUBLE_TAP_MS) {
                            lastTap[0] = 0L
                            setTo(slider, 0f, true)
                            return@awaitEachGesture
                        }
                        lastTap[0] = now
                        var db = at(down.position.y)
                        dragTo(slider to db)
                        setTo(slider, db, false)
                        drag(down.id) { change ->
                            change.consume()
                            db = at(change.position.y)
                            dragTo(slider to db)
                            setTo(slider, db, false)
                        }
                        setTo(slider, db, true)
                        dragTo(null)
                    }
                }
                .drawBehind {
                    val mid = size.height / 2
                    val y = (RANGE - value.coerceIn(-RANGE, RANGE)) / (2 * RANGE) * size.height
                    val cx = size.width / 2
                    val edge = 1.dp.toPx()
                    drawRect(Classic.hi, Offset(0f, mid), Size(size.width, edge))
                    val track = 6.dp.toPx()
                    drawRect(Color.Black, Offset(cx - track / 2, 0f), Size(track, size.height))
                    bevel(Offset(cx - track / 2, 0f), Size(track, size.height), edge, Classic.lo, Classic.hi)
                    val fill = 4.dp.toPx()
                    drawRect(
                        if (on) Classic.lcd else Classic.dim.copy(alpha = 0.55f),
                        Offset(cx - fill / 2, minOf(y, mid)), Size(fill, abs(y - mid)),
                    )
                    val thumb = Size(22.dp.toPx().coerceAtMost(size.width), 11.dp.toPx())
                    val at = Offset(cx - thumb.width / 2, y - thumb.height / 2)
                    val face = Classic.thumb
                    drawRect(face, at, thumb)
                    bevel(at, thumb, edge, lerp(face, Color.White, 0.45f), lerp(face, Color.Black, 0.6f))
                },
        )
        BasicText(mark, style = label(10.sp, Classic.label, 0.02.sp))
    }
}
