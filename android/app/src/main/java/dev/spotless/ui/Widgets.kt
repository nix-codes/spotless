package dev.spotless.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import dev.spotless.Core
import dev.spotless.core.PhosphorView

/**
 * Spotless Classic, the desktop's built-in skin (src/ui/theme.rs). The list
 * and display colours follow the colour chosen in Settings ([use]).
 */
object Classic {
    val frame = Color(0xFF2A2A37)
    val hi = Color(0xFF4C4C62)
    val lo = Color(0xFF111117)
    val chrome = Color(0xFFC9C9DA)
    val button = Color(0xFF363646)
    var lcd by mutableStateOf(Color(0xFF2BFF62))
        private set
    var list by mutableStateOf(Color(0xFF00E639))
        private set
    var dim by mutableStateOf(Color(0xFF14A83A))
        private set
    var current by mutableStateOf(Color.White)
        private set
    var selected by mutableStateOf(Color(0xFF0E4A1F))
        private set
    val thumb = Color(0xFFB0995E)
    /** Labels on the frame. */
    val label = Color(0xFF9D9DB3)
    /** Behind the playing song: a trace of the colour on near-black. */
    var playingRow by mutableStateOf(Color(0xFF0E1A10))
        private set
    /** Something went wrong, or waits: messages and the player's NO CONNECTION. */
    val warning = Color(0xFFFFB000)

    /** Draws the lists and the display in [phosphor]'s shades. */
    fun use(phosphor: PhosphorView) {
        fun rgb(value: UInt) = Color(0xFF000000.toInt() or value.toInt())
        lcd = rgb(phosphor.lit)
        list = rgb(phosphor.text)
        dim = rgb(phosphor.dim)
        current = rgb(phosphor.current)
        selected = rgb(phosphor.selected)
        playingRow = lerp(Color(0xFF0F0F10), list, 0.05f)
    }
}

/** A Winamp bevel: light top and left edges, dark bottom and right, or the other way in. */
fun Modifier.bevel(raised: Boolean = true, width: Dp = 2.dp) = drawBehind {
    val w = width.toPx()
    val (topLeft, bottomRight) = if (raised) Classic.hi to Classic.lo else Classic.lo to Classic.hi
    drawRect(topLeft, size = size.copy(height = w))
    drawRect(topLeft, size = size.copy(width = w))
    drawRect(bottomRight, topLeft = Offset(0f, size.height - w), size = size.copy(height = w))
    drawRect(bottomRight, topLeft = Offset(size.width - w, 0f), size = size.copy(width = w))
}

/** Black and sunk in: the display, the lists, the fields. */
fun Modifier.lcd() = background(Color.Black).bevel(raised = false)

fun label(size: TextUnit = 13.sp, color: Color = Classic.chrome, spacing: TextUnit = 0.08.em) =
    TextStyle(color = color, fontSize = size, fontWeight = FontWeight.SemiBold, letterSpacing = spacing)

fun body(size: TextUnit = 15.sp, color: Color = Classic.list, weight: FontWeight = FontWeight.Medium) =
    TextStyle(color = color, fontSize = size, fontWeight = weight)

fun mono(size: TextUnit = 15.sp, color: Color = Classic.list) =
    TextStyle(color = color, fontSize = size, fontFamily = FontFamily.Monospace)

@Composable
fun Line(text: String, style: TextStyle, modifier: Modifier = Modifier) {
    BasicText(text, modifier, style, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

/**
 * A chrome button: raised, or sunk and lit when [on] (a tab, a switch).
 * At least 44 dp each way, for a finger.
 */
/**
 * The last error, in full: it can be longer than a line holds. Tapping it
 * puts it away.
 */
@Composable
fun ErrorNote(core: Core, modifier: Modifier = Modifier) {
    val message = core.message?.takeIf { it.error && it.text != core.dismissed } ?: return
    BasicText(
        message.text,
        modifier
            .fillMaxWidth()
            .clickable { core.dismissed = message.text }
            .padding(horizontal = 16.dp, vertical = 6.dp),
        body(13.sp, Classic.warning, FontWeight.Normal),
    )
}

@Composable
fun ChromeButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    on: Boolean = false,
    enabled: Boolean = true,
    description: String? = null,
    onLongClick: (() -> Unit)? = null,
    content: @Composable BoxScope.() -> Unit,
) {
    Box(
        modifier
            .defaultMinSize(44.dp, 44.dp)
            .background(if (on) Color.Black else Classic.button)
            .bevel(raised = !on)
            .combinedClickable(enabled = enabled, role = Role.Button, onLongClick = onLongClick, onClick = onClick)
            .then(if (description != null) Modifier.semantics { contentDescription = description } else Modifier)
            .padding(horizontal = 12.dp),
        contentAlignment = Alignment.Center,
        content = content,
    )
}

@Composable
fun TextButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    on: Boolean = false,
    enabled: Boolean = true,
    lit: Boolean = on,
) {
    val color = when {
        !enabled -> Classic.label
        lit -> Classic.lcd
        else -> Classic.chrome
    }
    ChromeButton(onClick, modifier, on, enabled) { BasicText(text, style = label(color = color)) }
}

/** The transport's and the bars' glyphs, drawn in [color] on a 24-unit square. */
/** Heart is a song loved on Last.fm; Unloved, its outline, one that isn't. */
enum class Glyph { Previous, Play, Pause, Stop, Next, Search, Menu, Close, Back, Open, Closed, Check, Trash, Repeat, Shuffle, Lock, Share, Sync, Heart, Unloved }

@Composable
fun GlyphButton(
    glyph: Glyph,
    description: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    on: Boolean = false,
    lit: Boolean = on,
    onLongClick: (() -> Unit)? = null,
) {
    val color = when {
        !enabled -> Classic.label
        lit -> Classic.lcd
        else -> Classic.chrome
    }
    ChromeButton(onClick, modifier, on, enabled, description, onLongClick) {
        Box(Modifier.defaultMinSize(22.dp, 22.dp).drawBehind { glyph(glyph, color) })
    }
}

fun DrawScope.glyph(glyph: Glyph, color: Color) {
    val u = size.minDimension / 24f
    fun path(vararg points: Pair<Float, Float>) = Path().apply {
        points.forEachIndexed { i, (x, y) -> if (i == 0) moveTo(x * u, y * u) else lineTo(x * u, y * u) }
        close()
    }
    /** The same, left open, to stroke. */
    fun arm(vararg points: Pair<Float, Float>) = Path().apply {
        points.forEachIndexed { i, (x, y) -> if (i == 0) moveTo(x * u, y * u) else lineTo(x * u, y * u) }
    }
    fun bar(x: Float, y: Float, w: Float, h: Float) =
        drawRect(color, Offset(x * u, y * u), androidx.compose.ui.geometry.Size(w * u, h * u))
    val line = Stroke(width = 2.4f * u)
    when (glyph) {
        Glyph.Previous -> { bar(5f, 5f, 3f, 14f); drawPath(path(20f to 5f, 20f to 19f, 9f to 12f), color) }
        Glyph.Play -> drawPath(path(7f to 4f, 7f to 20f, 20f to 12f), color)
        Glyph.Pause -> { bar(6f, 5f, 4f, 14f); bar(14f, 5f, 4f, 14f) }
        Glyph.Stop -> bar(5f, 5f, 14f, 14f)
        Glyph.Next -> { bar(16f, 5f, 3f, 14f); drawPath(path(4f to 5f, 4f to 19f, 15f to 12f), color) }
        Glyph.Search -> {
            drawCircle(color, 6.5f * u, Offset(10.5f * u, 10.5f * u), style = line)
            drawLine(color, Offset(15.5f * u, 15.5f * u), Offset(21f * u, 21f * u), line.width)
        }
        Glyph.Menu -> for (y in listOf(7f, 12f, 17f)) {
            drawLine(color, Offset(4f * u, y * u), Offset(20f * u, y * u), line.width)
        }
        Glyph.Close -> {
            drawLine(color, Offset(6f * u, 6f * u), Offset(18f * u, 18f * u), line.width)
            drawLine(color, Offset(18f * u, 6f * u), Offset(6f * u, 18f * u), line.width)
        }
        Glyph.Back -> {
            drawLine(color, Offset(15f * u, 5f * u), Offset(8f * u, 12f * u), line.width)
            drawLine(color, Offset(8f * u, 12f * u), Offset(15f * u, 19f * u), line.width)
        }
        Glyph.Open -> drawPath(path(4f to 7f, 20f to 7f, 12f to 17f), color)
        Glyph.Closed -> drawPath(path(8f to 4f, 8f to 20f, 17f to 12f), color)
        Glyph.Check -> {
            drawLine(color, Offset(5f * u, 12.5f * u), Offset(10f * u, 17.5f * u), line.width)
            drawLine(color, Offset(10f * u, 17.5f * u), Offset(19f * u, 6.5f * u), line.width)
        }
        Glyph.Trash -> {
            // The lid and its handle, the can, and two ribs down it.
            drawLine(color, Offset(4f * u, 6.5f * u), Offset(20f * u, 6.5f * u), line.width)
            val handle = Path().apply {
                moveTo(9.5f * u, 6.5f * u)
                lineTo(9.5f * u, 3.5f * u)
                lineTo(14.5f * u, 3.5f * u)
                lineTo(14.5f * u, 6.5f * u)
            }
            drawPath(handle, color, style = line)
            drawPath(path(6.5f to 9.5f, 7.5f to 20.5f, 16.5f to 20.5f, 17.5f to 9.5f), color, style = line)
            for (x in listOf(10.5f, 13.5f)) {
                drawLine(color, Offset(x * u, 12.5f * u), Offset(x * u, 17.5f * u), 1.8f * u)
            }
        }
        Glyph.Lock -> {
            // A padlock: its body, and the shackle's arch over it.
            bar(5.5f, 11f, 13f, 10f)
            drawPath(arm(8.5f to 11f, 8.5f to 8f, 9.5f to 5.5f, 12f to 4.5f, 14.5f to 5.5f, 15.5f to 8f, 15.5f to 11f), color, style = line)
        }
        Glyph.Sync -> {
            // A synced playlist's mark (⇅): an arrow up beside one down.
            drawLine(color, Offset(8f * u, 20f * u), Offset(8f * u, 5f * u), 1.8f * u)
            drawPath(arm(4f to 9f, 8f to 4.5f, 12f to 9f), color, style = line)
            drawLine(color, Offset(16f * u, 4f * u), Offset(16f * u, 19f * u), 1.8f * u)
            drawPath(arm(12f to 15f, 16f to 19.5f, 20f to 15f), color, style = line)
        }
        Glyph.Share -> {
            // Three dots, the first passing on to the other two.
            val from = Offset(6f * u, 12f * u)
            for (to in listOf(Offset(18f * u, 5.5f * u), Offset(18f * u, 18.5f * u))) {
                drawLine(color, from, to, 1.8f * u)
                drawCircle(color, 3f * u, to)
            }
            drawCircle(color, 3f * u, from)
        }
        Glyph.Heart, Glyph.Unloved -> {
            // Two round lobes, meeting in a point below.
            val heart = Path().apply {
                moveTo(12f * u, 20.5f * u)
                cubicTo(5f * u, 15.5f * u, 2.5f * u, 12f * u, 2.5f * u, 8.5f * u)
                cubicTo(2.5f * u, 5.5f * u, 4.8f * u, 3.5f * u, 7.5f * u, 3.5f * u)
                cubicTo(9.5f * u, 3.5f * u, 11f * u, 4.6f * u, 12f * u, 6.2f * u)
                cubicTo(13f * u, 4.6f * u, 14.5f * u, 3.5f * u, 16.5f * u, 3.5f * u)
                cubicTo(19.2f * u, 3.5f * u, 21.5f * u, 5.5f * u, 21.5f * u, 8.5f * u)
                cubicTo(21.5f * u, 12f * u, 19f * u, 15.5f * u, 12f * u, 20.5f * u)
                close()
            }
            if (glyph == Glyph.Heart) drawPath(heart, color) else drawPath(heart, color, style = Stroke(width = 1.8f * u))
        }
        Glyph.Repeat -> {
            // Two arrows chasing each other round a loop.
            drawPath(arm(4.5f to 13f, 4.5f to 8f, 16f to 8f), color, style = line)
            drawPath(path(15.5f to 4f, 20.5f to 8f, 15.5f to 12f), color)
            drawPath(arm(19.5f to 11f, 19.5f to 16f, 8f to 16f), color, style = line)
            drawPath(path(8.5f to 12f, 3.5f to 16f, 8.5f to 20f), color)
        }
        Glyph.Shuffle -> {
            // Two arrows crossing on their way right.
            drawPath(arm(3.5f to 7f, 8f to 7f, 14.5f to 17f, 17f to 17f), color, style = line)
            drawPath(path(16f to 13f, 21f to 17f, 16f to 21f), color)
            drawPath(arm(3.5f to 17f, 8f to 17f, 14.5f to 7f, 17f to 7f), color, style = line)
            drawPath(path(16f to 3f, 21f to 7f, 16f to 11f), color)
        }
    }
}

/**
 * The name, as on the desktop's title bar: the icon's LCD, SPOTLESS, and
 * the bar's grooves filling the rest of [modifier]'s width.
 */
@Composable
fun Logo(modifier: Modifier = Modifier) {
    Row(
        modifier.semantics { contentDescription = "Spotless" },
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(26.dp).lcd().drawBehind { mark() })
        BasicText(
            "SPOTLESS",
            style = TextStyle(
                color = Classic.chrome,
                fontSize = 17.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                letterSpacing = 0.18.em,
            ),
        )
        Box(Modifier.weight(1f).height(10.dp).drawBehind { grooves() })
    }
}

/** The launcher icon's play sign and bars, from its 56-unit LCD. */
private fun DrawScope.mark() {
    val u = size.minDimension / 56f
    fun bar(color: Color, x: Float, top: Float) =
        drawRect(color, Offset(x * u, top * u), androidx.compose.ui.geometry.Size(5.33f * u, (48f - top) * u))
    // Pulled in a little, off the bevel.
    translate(top = -1f * u) {
        drawPath(
            Path().apply {
                moveTo(8.67f * u, 11.33f * u)
                lineTo(28f * u, 22.67f * u)
                lineTo(8.67f * u, 34f * u)
                close()
            },
            Classic.lcd,
        )
        bar(Classic.lcd, 33.33f, 26f)
        bar(Classic.lcd, 41.33f, 17.33f)
        for ((x, top) in listOf(8.67f to 39.33f, 16.67f to 35.33f, 24.67f to 31.33f)) bar(Classic.dim, x, top)
    }
}

/** The title bar's grooves: thin light lines, 3 dp apart. */
private fun DrawScope.grooves() {
    val step = 3.dp.toPx()
    var y = 0.5f
    while (y <= size.height) {
        drawLine(Classic.hi, Offset(0f, y), Offset(size.width, y), 1.dp.toPx())
        y += step
    }
}

/** m:ss, or h:mm:ss from an hour. */
fun clock(ms: Long): String {
    val total = ms / 1000
    val (h, m, s) = Triple(total / 3600, total / 60 % 60, total % 60)
    return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(total / 60, s)
}

fun songs(n: Int) = if (n == 1) "1 SONG" else "$n SONGS"
