package dev.spotless.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.AlbumSongs
import dev.spotless.core.OpenedCode
import dev.spotless.core.ReleaseRow

/** The tallest bar, as the logo's width is: the rest are fractions of it, as on the desktop. */
private const val BAR = 5f / 44f
private const val PITCH = 8.6f / 44f
private const val GAP = 12f / 44f
/** The whole code's width, in heights. */
private const val WIDTH = 1f + GAP + PITCH * 22f + BAR

/**
 * The Spotify Code the page was opened from, drawn again from its bars and
 * centred, so it's plain where the page came from: with the album's or
 * song's title under it on its artist's page. Held, it shares its link.
 */
@Composable
internal fun OpenedCodeView(core: Core, code: OpenedCode) {
    Column(
        Modifier
            .fillMaxWidth()
            .combinedClickable(onLongClick = { core.share(listOf(code.uri)) }) {}
            .padding(vertical = 12.dp)
            .semantics { contentDescription = "Opened from this Spotify Code. Hold to share its link" },
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        val height = 40.dp
        Canvas(Modifier.size(height * WIDTH, height)) { codeBars(code.bars, Classic.dim) }
        code.title?.let {
            BasicText("CODE · “$it”", Modifier.padding(top = 8.dp), label(12.sp, Classic.dim))
        }
    }
}

/** A Spotify Code's logo dot and 23 bars (heights 0 to 7), filling the height. */
private fun DrawScope.codeBars(bars: ByteArray, color: Color) {
    val h = size.height
    val middle = h / 2
    drawCircle(color, h / 2, Offset(h / 2, middle))
    val bar = BAR * h
    bars.forEachIndexed { i, level ->
        val x = h + GAP * h + PITCH * h * i
        val tall = bar + (h - bar) * level.toInt() / 7f
        drawRoundRect(color, Offset(x, middle - tall / 2), Size(bar, tall), CornerRadius(bar / 2))
    }
}

/**
 * Scrolls an artist's page to the album or song a code or link opened, once
 * it's in the list (a song once its album's songs are read), then tells the
 * core. [rows] are the releases shown, the list's last items.
 */
@Composable
internal fun Reveal(core: Core, scroll: LazyListState, rows: List<ReleaseRow>) {
    val reveal = core.browse.reveal ?: return
    // Where it is among the releases' items, and how many those are.
    var at: Int? = null
    var items = 0
    for (row in rows) {
        if (at == null && row.release.uri == reveal) at = items
        items += 1
        if (!row.expanded) continue
        when (val songs = row.songs) {
            is AlbumSongs.Ready -> {
                val i = songs.tracks.indexOfFirst { it.uri == reveal }
                if (at == null && i >= 0) at = items + i
                items += songs.tracks.size
            }
            else -> items += 1
        }
    }
    val found = at ?: return
    LaunchedEffect(reveal, found, items) {
        // Once laid out: what comes before the releases is the rest.
        withFrameNanos {}
        val before = scroll.layoutInfo.totalItemsCount - items
        // One row above it shows too, for where it is.
        scroll.scrollToItem((before + found - 1).coerceAtLeast(0))
        core.apply(Action.Revealed)
    }
}
