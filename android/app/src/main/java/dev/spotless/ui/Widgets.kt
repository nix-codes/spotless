package dev.spotless.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
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

/** Spotless Classic, the desktop's built-in skin (src/ui/theme.rs). */
object Classic {
    val frame = Color(0xFF2A2A37)
    val hi = Color(0xFF4C4C62)
    val lo = Color(0xFF111117)
    val chrome = Color(0xFFC9C9DA)
    val button = Color(0xFF363646)
    val lcd = Color(0xFF2BFF62)
    val list = Color(0xFF00E639)
    val dim = Color(0xFF14A83A)
    val current = Color.White
    val selected = Color(0xFF0B2FBF)
    val thumb = Color(0xFFB0995E)
    /** Labels on the frame. */
    val label = Color(0xFF9D9DB3)
    val playingRow = Color(0xFF101A12)
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
enum class Glyph { Previous, Play, Pause, Stop, Next, Search, Menu, Close, Back, Open, Closed, Check, Trash }

@Composable
fun GlyphButton(
    glyph: Glyph,
    description: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val color = if (enabled) Classic.chrome else Classic.label
    ChromeButton(onClick, modifier, enabled = enabled, description = description) {
        Box(Modifier.defaultMinSize(22.dp, 22.dp).drawBehind { glyph(glyph, color) })
    }
}

fun DrawScope.glyph(glyph: Glyph, color: Color) {
    val u = size.minDimension / 24f
    fun path(vararg points: Pair<Float, Float>) = Path().apply {
        points.forEachIndexed { i, (x, y) -> if (i == 0) moveTo(x * u, y * u) else lineTo(x * u, y * u) }
        close()
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
    }
}

/** m:ss, or h:mm:ss from an hour. */
fun clock(ms: Long): String {
    val total = ms / 1000
    val (h, m, s) = Triple(total / 3600, total / 60 % 60, total % 60)
    return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(total / 60, s)
}

fun songs(n: Int) = if (n == 1) "1 SONG" else "$n SONGS"
