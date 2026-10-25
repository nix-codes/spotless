package dev.spotless.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.basicMarquee
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.Player
import dev.spotless.core.Session
import dev.spotless.core.Status
import kotlinx.coroutines.delay

/** The display's title line: the song, or what the player is doing. */
private fun title(core: Core): String {
    val track = core.player.track
    return when {
        track != null -> "${track.artists} - ${track.title}"
        core.session == Session.Connecting -> "CONNECTING…"
        else -> ""
    }
}

/** The display's time: where the song is, or dashes with none loaded. */
private fun time(player: Player) = if (player.track == null) "-:--" else clock(player.positionMs.toLong())

/**
 * The time, whose digits blink while paused, as in Winamp (and the
 * desktop); the colon stays.
 */
@Composable
private fun shownTime(player: Player): String {
    val text = time(player)
    val paused = player.status == Status.PAUSED && player.track != null
    var lit by remember { mutableStateOf(true) }
    LaunchedEffect(paused) {
        lit = true
        while (paused) {
            delay(BLINK_MS)
            lit = !lit
        }
    }
    return if (!paused || lit) text else text.map { if (it == ':') it else ' ' }.joinToString("")
}

private const val BLINK_MS = 1000L

/** A line of the display that, too long to fit, scrolls slowly round and round. */
@Composable
private fun Scrolling(text: String, style: TextStyle, modifier: Modifier = Modifier) {
    BasicText(
        text,
        modifier.basicMarquee(iterations = Int.MAX_VALUE, repeatDelayMillis = 3500, velocity = 20.dp),
        style,
        maxLines = 1,
    )
}

private fun playing(player: Player) = player.status == Status.PLAYING || player.status == Status.LOADING

/** Tapped, the song shows in its playlist. */
private fun Modifier.showsSong(core: Core) = clickable(
    enabled = core.player.track != null && core.player.playlist != null,
    onClickLabel = "Show the song in its playlist",
    onClick = core::jumpToPlaying,
)

/** The player under the playlists: display, seek bar, transport, SHUF and REP. */
@Composable
fun PlayerPanel(core: Core) {
    val player = core.player
    val ready = core.session is Session.Ready
    Column(
        Modifier.padding(start = 8.dp, end = 8.dp, top = 10.dp, bottom = 14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(
            Modifier.fillMaxWidth().showsSong(core).lcd().padding(horizontal = 12.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            BasicText(shownTime(player), style = mono(40.sp, Classic.lcd))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Scrolling(title(core), body(color = Classic.lcd))
                if (player.offline) {
                    Line("NO CONNECTION · PAUSED", mono(13.sp, Classic.warning))
                } else {
                    val info = player.track?.let { "320 KBPS · 44 KHZ · " + clock(it.durationMs.toLong()) } ?: ""
                    Line(info, mono(13.sp, Classic.dim))
                }
            }
        }
        SeekBar(player) { core.apply(Action.Seek(it)) }
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            val wide = Modifier.weight(1f).height(52.dp)
            GlyphButton(Glyph.Previous, "Previous", { core.apply(Action.Previous) }, wide, ready)
            // Play stays pressed in while a song plays, Pause while it is paused; Stop never.
            GlyphButton(Glyph.Play, "Play", { core.apply(Action.Play) }, wide, ready, on = playing(player))
            GlyphButton(
                Glyph.Pause, "Pause", { core.apply(Action.TogglePause) }, wide, ready,
                on = player.status == Status.PAUSED,
            )
            GlyphButton(Glyph.Stop, "Stop", { core.apply(Action.Stop) }, wide, ready)
            GlyphButton(Glyph.Next, "Next", { core.apply(Action.Next) }, wide, ready)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            GlyphButton(
                Glyph.Shuffle, if (player.shuffle) "Shuffle, on" else "Shuffle, off",
                { core.apply(Action.SetShuffle(!player.shuffle)) },
                Modifier.weight(1f), lit = player.shuffle,
            )
            GlyphButton(
                Glyph.Repeat, if (player.repeat) "Repeat, on" else "Repeat, off",
                { core.apply(Action.SetRepeat(!player.repeat)) },
                Modifier.weight(1f), lit = player.repeat,
            )
        }
    }
}

/** Tap or drag to a place in the song; it seeks when let go. */
@Composable
private fun SeekBar(player: Player, seek: (UInt) -> Unit) {
    val length = player.track?.durationMs?.toFloat() ?: 0f
    var width by remember { mutableStateOf(1) }
    var dragged by remember { mutableStateOf<Float?>(null) }
    val shown = dragged ?: if (length > 0) player.positionMs.toFloat() / length else 0f
    val fraction = shown.coerceIn(0f, 1f)
    fun at(x: Float) = (x / width).coerceIn(0f, 1f)
    Box(
        Modifier
            .fillMaxWidth()
            .height(28.dp)
            .onSizeChanged { width = it.width.coerceAtLeast(1) }
            .semantics { contentDescription = "Position in the song" }
            .pointerInput(length) {
                if (length <= 0) return@pointerInput
                detectTapGestures { seek((at(it.x) * length).toUInt()) }
            }
            .pointerInput(length) {
                if (length <= 0) return@pointerInput
                detectHorizontalDragGestures(
                    onDragStart = { dragged = at(it.x) },
                    onDragEnd = {
                        dragged?.let { seek((it * length).toUInt()) }
                        dragged = null
                    },
                    onDragCancel = { dragged = null },
                ) { change, _ -> dragged = at(change.position.x) }
            },
        contentAlignment = Alignment.CenterStart,
    ) {
        Box(Modifier.fillMaxWidth().height(10.dp).lcd()) {
            Box(Modifier.fillMaxWidth(fraction).fillMaxHeight().background(Classic.dim))
        }
        val thumb = 24.dp
        val left = with(LocalDensity.current) { (fraction * width).toDp() } - thumb / 2
        Box(
            Modifier
                .offset(x = left.coerceAtLeast(0.dp))
                .size(thumb)
                .background(Classic.chrome)
                .bevel(),
        )
    }
}

/** Browse's player: the time, the song, and play or pause. */
@Composable
fun MiniPlayer(core: Core) {
    val player = core.player
    Row(
        Modifier.fillMaxWidth().height(52.dp).lcd().padding(start = 12.dp, end = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(shownTime(player), style = mono(24.sp, Classic.lcd))
        Scrolling(title(core), body(14.sp, Classic.lcd), Modifier.weight(1f).showsSong(core))
        if (playing(player)) {
            GlyphButton(Glyph.Pause, "Pause", { core.apply(Action.TogglePause) })
        } else {
            GlyphButton(Glyph.Play, "Play", { core.apply(Action.Play) }, enabled = core.session is Session.Ready)
        }
    }
}
