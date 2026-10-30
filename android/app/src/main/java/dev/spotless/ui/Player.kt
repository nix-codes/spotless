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
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.TextUnit
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

/**
 * The display's time: where the song is, or with [left] what's left of it
 * after a minus, counted up so the song starts at its length and ends at 0;
 * dashes with none loaded.
 */
private fun time(player: Player, left: Boolean): String {
    val track = player.track ?: return "-:--"
    if (!left) return clock(player.positionMs.toLong())
    val rest = (track.durationMs.toLong() - player.positionMs.toLong()).coerceAtLeast(0)
    return "-" + clock((rest + 999) / 1000 * 1000)
}

/**
 * The time, whose digits blink while paused, as in Winamp (and the
 * desktop); the colon and the minus stay.
 */
@Composable
private fun shownTime(player: Player, left: Boolean): String {
    val text = time(player, left)
    val paused = player.status == Status.PAUSED && player.track != null
    var lit by remember { mutableStateOf(true) }
    LaunchedEffect(paused) {
        lit = true
        while (paused) {
            delay(BLINK_MS)
            lit = !lit
        }
    }
    return if (!paused || lit) text else text.map { if (it == ':' || it == '-') it else ' ' }.joinToString("")
}

private const val BLINK_MS = 1000L

/** The time; tapped, it shows what's left of the song, or again what has played. */
@Composable
private fun Time(core: Core, size: TextUnit) {
    val left = core.settings.timeLeft
    BasicText(
        shownTime(core.player, left),
        Modifier.clickable(
            onClickLabel = if (left) "Show the time played" else "Show the time left",
            onClick = { core.apply(Action.ToggleTimeLeft) },
        ),
        mono(size, Classic.lcd),
    )
}

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
            Time(core, 40.sp)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Scrolling(title(core), body(color = Classic.lcd))
                if (player.offline) {
                    Line("NO CONNECTION · PAUSED", mono(13.sp, Classic.warning))
                } else {
                    val info = player.track?.let { "320 KBPS · 44 KHZ · " + clock(it.durationMs.toLong()) } ?: ""
                    Line(info, mono(13.sp, Classic.dim))
                }
            }
            Heart(core)
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
            // Held: stops once the song ends (Winamp's Stop after current), pressed in
            // with Play till then, as on the desktop.
            GlyphButton(
                Glyph.Stop, if (player.stopAfter) "Stop, stopping after this song" else "Stop; hold to stop after this song",
                { core.apply(Action.Stop) }, wide, ready,
                on = player.stopAfter,
                onLongClick = { core.apply(Action.ToggleStopAfter) },
            )
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

/**
 * While Last.fm is connected, the display's heart: full while the song is
 * loved there; a tap loves it or unloves it.
 */
@Composable
private fun Heart(core: Core) {
    val player = core.player
    val loved = player.loved ?: return
    val track = player.track ?: return
    Box(
        Modifier
            .size(44.dp)
            .clickable(onClickLabel = if (loved) "Unlove on Last.fm" else "Love on Last.fm") {
                core.apply(Action.SetLoved(track.mainArtist, track.title, !loved))
            }
            .semantics { contentDescription = if (loved) "Loved on Last.fm" else "Not loved on Last.fm" },
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.size(24.dp).drawBehind { glyph(if (loved) Glyph.Heart else Glyph.Unloved, if (loved) Classic.lcd else Classic.dim) })
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
        Time(core, 24.sp)
        Scrolling(title(core), body(14.sp, Classic.lcd), Modifier.weight(1f).showsSong(core))
        if (playing(player)) {
            GlyphButton(Glyph.Pause, "Pause", { core.apply(Action.TogglePause) })
        } else {
            GlyphButton(Glyph.Play, "Play", { core.apply(Action.Play) }, enabled = core.session is Session.Ready)
        }
    }
}
