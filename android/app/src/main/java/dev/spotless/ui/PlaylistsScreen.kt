package dev.spotless.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.runtime.Composable
import kotlinx.coroutines.delay
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import androidx.compose.ui.zIndex
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.Played
import dev.spotless.core.Song
import dev.spotless.core.SpotifyRow
import dev.spotless.core.SpotifyView
import dev.spotless.core.Track
import kotlin.math.abs

private val ROW = 52.dp
private val HEADER = 30.dp

/**
 * The main screen: the playlist tabs, the viewed playlist, and the player.
 * Holding a song selects it, for Q NEXT, copying, removing and moving;
 * holding another then selects every song from the last one picked up to
 * it. Each album's run of songs sits under a header with its name.
 *
 * The Spotify tab, while Spotless is in Spotify Connect, shows the
 * account's queue instead: what played, the song playing, then what comes.
 */
@Composable
fun PlaylistsScreen(core: Core, openSettings: () -> Unit) {
    val queue = viewedQueue(core)
    // Entries of the viewed playlist; ones since removed drop out.
    var picked by remember(core.tabs.viewed) { mutableStateOf(emptySet<ULong>()) }
    val entries = if (queue != null) queueSongs(queue).map { it.entry } else core.playlist?.songs?.map { it.entry }.orEmpty()
    val selected = entries.filter { it in picked }
    val selecting = selected.isNotEmpty()
    var copying by remember(core.tabs.viewed) { mutableStateOf(false) }
    var editing by remember { mutableStateOf<ULong?>(null) }
    var adding by remember { mutableStateOf(false) }
    val done = {
        picked = emptySet()
        copying = false
    }
    BackHandler(enabled = selecting) { done() }
    BackHandler(enabled = editing != null) { editing = null }
    BackHandler(enabled = adding) { adding = false }
    Column(Modifier.fillMaxSize().background(Classic.frame).safeDrawingPadding()) {
        if (selecting) {
            SelectBar(core, queue, selected, { picked = entries.toSet() }, { copying = !copying }, copying, done)
            if (copying) CopyBar(core, selectedTracks(core, queue, selected), done)
        } else {
            TopBar(core, openSettings)
        }
        Tabs(core, { editing = it }, { adding = !adding })
        editing?.let { TabEditor(core, it) { editing = null } }
        if (adding && core.settings.spotifyTabClosed) AddBar(core) { adding = false }
        Summary(core, queue, selecting)
        ErrorNote(core)
        if (queue != null) {
            QueueSongs(core, queue, selected, { picked = it }, Modifier.weight(1f))
        } else {
            Songs(core, selected, { picked = it }, Modifier.weight(1f))
        }
        if (selecting) {
            Box(Modifier.padding(start = 8.dp, end = 8.dp, top = 8.dp, bottom = 14.dp)) { MiniPlayer(core) }
        } else {
            PlayerPanel(core)
        }
    }
}

@Composable
private fun TopBar(core: Core, openSettings: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(56.dp).padding(start = 16.dp, end = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Logo(Modifier.weight(1f).padding(end = 4.dp))
        GlyphButton(Glyph.Search, "Browse", { core.apply(Action.OpenBrowse) }, Modifier.width(48.dp))
        GlyphButton(Glyph.Menu, "Settings", openSettings, Modifier.width(48.dp))
    }
}

/** The Spotify tab's queue, when that is the tab viewed. */
private fun viewedQueue(core: Core): SpotifyView? =
    core.spotify?.takeIf { core.tabs.tabs.any { it.id == core.tabs.viewed && it.spotify } }

/** The queue's songs, without the headings between them. */
private fun queueSongs(queue: SpotifyView) = queue.rows.filterIsInstance<SpotifyRow.Song>()

/** The selected songs' tracks, in the tab's order. */
private fun selectedTracks(core: Core, queue: SpotifyView?, selected: List<ULong>): List<Track> =
    if (queue != null) {
        queueSongs(queue).filter { it.entry in selected }.map { it.track }
    } else {
        core.playlist?.songs?.filter { it.entry in selected }?.map { it.track }.orEmpty()
    }

/** In place of the top bar while songs are selected. */
@Composable
private fun SelectBar(
    core: Core,
    queue: SpotifyView?,
    selected: List<ULong>,
    all: () -> Unit,
    copy: () -> Unit,
    copying: Boolean,
    done: () -> Unit,
) {
    val id = core.tabs.viewed
    val count = if (queue != null) queueSongs(queue).size else core.playlist?.songs?.size ?: return
    // Of the queue, only songs to come can go, and only while Spotless has it.
    val removable = if (queue == null) {
        selected
    } else if (queue.heldHere) {
        queueSongs(queue).filter { it.entry in selected && it.played == Played.NEXT }.map { it.entry }
    } else {
        emptyList()
    }
    Row(
        Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GlyphButton(Glyph.Close, "Stop selecting", done, Modifier.width(48.dp))
        Line(
            "${selected.size} selected",
            body(15.sp, Classic.current, FontWeight.SemiBold),
            Modifier.weight(1f).padding(start = 4.dp),
        )
        TextButton("ALL", all, enabled = selected.size < count)
        if (queue == null) TextButton("Q NEXT", { core.apply(Action.ToggleMarks(selected)) })
        TextButton("COPY", copy, on = copying)
        GlyphButton(Glyph.Trash, if (queue != null) "Remove from the queue" else "Remove from the playlist", {
            core.apply(Action.Remove(id, removable))
            done()
        }, Modifier.width(48.dp), enabled = removable.isNotEmpty())
    }
}

/** COPY's choice of tab: the selected songs go to the end of the one tapped. */
@Composable
private fun CopyBar(core: Core, tracks: List<Track>, done: () -> Unit) {
    val queue = core.spotify
    Column(
        Modifier.fillMaxWidth().padding(8.dp).background(Classic.frame).bevel().padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        BasicText("COPY TO", style = label(12.sp, Classic.label, 0.06.em))
        LazyRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            items(core.tabs.tabs.filter { it.id != core.tabs.viewed }, key = { it.id.toLong() }) { tab ->
                // Spotify takes songs for the queue only from the device playing it.
                TextButton(tab.name, {
                    core.apply(Action.CopyTo(tab.id, tracks))
                    done()
                }, Modifier.height(44.dp), enabled = !tab.spotify || queue?.takesSongs == true)
            }
        }
    }
}

/** A tab being dragged: where it was, its tab's left edges then, and how far it has moved. */
private class TabDrag(val id: ULong, val from: Int, val lefts: List<Float>, val widths: List<Float>) {
    var dx by mutableFloatStateOf(0f)

    /** Where it would go: past a tab once its edge reaches that tab's middle. */
    fun target(): Int {
        val left = lefts[from] + dx
        val right = left + widths[from]
        val middle = { i: Int -> lefts[i] + widths[i] / 2 }
        val after = (from + 1 until lefts.size).count { right > middle(it) }
        val before = (0 until from).count { left < middle(it) }
        return from + after - before
    }
}

/**
 * The playlists' tabs. Holding one opens its editor; holding it and sliding
 * moves it, the others making room.
 */
@Composable
private fun Tabs(core: Core, edit: (ULong) -> Unit, add: () -> Unit) {
    val tabs = core.tabs.tabs
    val state = rememberLazyListState()
    var drag by remember { mutableStateOf<TabDrag?>(null) }
    val gap = with(LocalDensity.current) { 4.dp.toPx() }
    val haptics = LocalHapticFeedback.current
    LazyRow(
        Modifier.fillMaxWidth().height(44.dp),
        state = state,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        contentPadding = PaddingValues(horizontal = 8.dp),
    ) {
        itemsIndexed(tabs, key = { _, tab -> tab.id.toLong() }) { index, tab ->
            val on = tab.id == core.tabs.viewed
            val held = drag?.takeIf { it.id == tab.id }
            // The others slide aside to make room.
            val room = drag?.let { d ->
                val to = d.target()
                val width = d.widths[d.from] + gap
                when {
                    d.id == tab.id -> 0f
                    index in (d.from + 1)..to -> -width
                    index in to until d.from -> width
                    else -> 0f
                }
            } ?: 0f
            val slide by animateFloatAsState(
                room,
                if (drag != null) tween(120) else snap(),
                label = "tab",
            )
            ChromeButton(
                { core.apply(Action.SelectPlaylist(tab.id)) },
                Modifier
                    .fillMaxHeight()
                    .zIndex(if (held != null) 1f else 0f)
                    .graphicsLayer { translationX = held?.dx ?: slide }
                    .pointerInput(tab.id, index, tabs.size) {
                        detectDragGesturesAfterLongPress(
                            onDragStart = {
                                haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                                val items = state.layoutInfo.visibleItemsInfo
                                    .filter { it.index < tabs.size }
                                    .sortedBy { it.index }
                                // Every tab is laid out when they fit; a strip
                                // longer than the screen moves what it shows.
                                if (items.size == tabs.size) {
                                    drag = TabDrag(
                                        tab.id,
                                        index,
                                        items.map { it.offset.toFloat() },
                                        items.map { it.size.toFloat() },
                                    )
                                }
                            },
                            onDrag = { change, amount ->
                                change.consume()
                                drag?.let { d ->
                                    val left = d.lefts[d.from]
                                    val end = d.lefts.last() + d.widths.last() - d.widths[d.from]
                                    d.dx = (d.dx + amount.x).coerceIn(d.lefts.first() - left, end - left)
                                }
                            },
                            onDragEnd = {
                                val d = drag
                                drag = null
                                when {
                                    d == null -> edit(tab.id)
                                    abs(d.dx) < 8f -> edit(tab.id)
                                    d.target() != d.from -> core.apply(Action.MovePlaylist(tab.id, d.target().toUInt()))
                                }
                            },
                            onDragCancel = { drag = null },
                        )
                    },
                on = on,
            ) {
                BasicText(tab.name, style = body(14.sp, if (on) Classic.lcd else Classic.chrome))
            }
        }
        item {
            // With the Spotify tab closed, + opens it too.
            val closed = core.settings.spotifyTabClosed
            ChromeButton(
                { if (closed) add() else core.apply(Action.NewPlaylist) },
                Modifier.fillMaxHeight(),
                description = if (closed) "New playlist or the Spotify tab" else "New playlist",
            ) {
                BasicText("+", style = label(20.sp))
            }
        }
    }
}

/** What + offers while the Spotify tab is closed. */
@Composable
private fun AddBar(core: Core, done: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(8.dp).background(Classic.frame).bevel().padding(8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TextButton("NEW PLAYLIST", {
            core.apply(Action.NewPlaylist)
            done()
        })
        TextButton("SPOTIFY", {
            core.apply(Action.OpenSpotifyTab)
            done()
        })
        Box(Modifier.weight(1f))
        GlyphButton(Glyph.Close, "Done", done, Modifier.width(48.dp))
    }
}

/** A held tab's name, place, emptying it, and deleting it (never Default's). */
@Composable
private fun TabEditor(core: Core, id: ULong, done: () -> Unit) {
    val tabs = core.tabs.tabs
    val at = tabs.indexOfFirst { it.id == id }
    if (at < 0) {
        // Deleted meanwhile.
        LaunchedEffect(id) { done() }
        return
    }
    val tab = tabs[at]
    if (tab.spotify) return SpotifyTabEditor(core, id, at, done)
    var name by remember(id) { mutableStateOf(tab.name) }
    // Clearing and deleting each ask once more, on the same button.
    var clearing by remember(id) { mutableStateOf(false) }
    var deleting by remember(id) { mutableStateOf(false) }
    val focus = LocalFocusManager.current
    val save = {
        if (name.isNotBlank() && name.trim() != tab.name) core.apply(Action.RenamePlaylist(id, name))
        focus.clearFocus()
        done()
    }
    Column(
        Modifier.fillMaxWidth().padding(8.dp).background(Classic.frame).bevel().padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            BasicTextField(
                name,
                { name = it },
                Modifier
                    .weight(1f)
                    .height(44.dp)
                    .lcd()
                    .padding(horizontal = 12.dp, vertical = 12.dp)
                    .semantics { contentDescription = "Playlist name" },
                textStyle = body(16.sp, Classic.lcd),
                singleLine = true,
                cursorBrush = SolidColor(Classic.lcd),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { save() }),
            )
            TextButton("RENAME", save, enabled = name.isNotBlank() && name.trim() != tab.name)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TextButton("◀", { core.apply(Action.MovePlaylist(id, (at - 1).toUInt())) }, enabled = at > 0)
            TextButton("▶", { core.apply(Action.MovePlaylist(id, (at + 1).toUInt())) }, enabled = at < tabs.lastIndex)
            Box(Modifier.weight(1f))
            if (!deleting) {
                TextButton(if (clearing) "CLEAR ${songs(tab.songs.toInt())}?" else "CLEAR", {
                    if (clearing) {
                        core.clear(id)
                        done()
                    } else {
                        clearing = true
                    }
                }, enabled = tab.songs > 0u, lit = clearing)
            }
            if (id != 0UL && !clearing) {
                TextButton(if (deleting) "DELETE ${songs(tab.songs.toInt())}?" else "DELETE", {
                    // An empty playlist has nothing to lose: no second ask.
                    if (deleting || tab.songs == 0u) {
                        core.apply(Action.ClosePlaylist(id))
                        done()
                    } else {
                        deleting = true
                    }
                }, lit = deleting)
            }
            GlyphButton(Glyph.Close, "Done", {
                focus.clearFocus()
                done()
            }, Modifier.width(48.dp))
        }
    }
}

/**
 * The Spotify tab's place, and closing it: Spotless leaves Spotify Connect,
 * and the queue goes on wherever it plays.
 */
@Composable
private fun SpotifyTabEditor(core: Core, id: ULong, at: Int, done: () -> Unit) {
    val tabs = core.tabs.tabs
    Column(
        Modifier.fillMaxWidth().padding(8.dp).background(Classic.frame).bevel().padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        BasicText(
            "Your account's queue, with Spotless as a Spotify Connect device. Closed, Spotless leaves Connect; + opens it again.",
            style = body(13.sp, Classic.label, FontWeight.Normal),
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TextButton("◀", { core.apply(Action.MovePlaylist(id, (at - 1).toUInt())) }, enabled = at > 0)
            TextButton("▶", { core.apply(Action.MovePlaylist(id, (at + 1).toUInt())) }, enabled = at < tabs.lastIndex)
            Box(Modifier.weight(1f))
            TextButton("CLOSE", {
                core.apply(Action.ClosePlaylist(id))
                done()
            })
            GlyphButton(Glyph.Close, "Done", done, Modifier.width(48.dp))
        }
    }
}

@Composable
private fun Summary(core: Core, queue: SpotifyView?, selecting: Boolean) {
    val list = core.playlist
    val tab = core.tabs.tabs.firstOrNull { it.id == core.tabs.viewed }
    val text = when {
        selecting && queue != null -> "TAP ADD/DROP · HOLD RANGE"
        selecting -> "TAP ADD/DROP · HOLD RANGE · DRAG ≡ MOVE"
        queue != null && tab != null -> "${songs(queue.toCome.toInt())} TO COME · ${clock(tab.durationMs.toLong())}"
        list != null -> "${songs(list.songs.size)} · ${clock(list.durationMs.toLong())}"
        else -> ""
    }
    // CLEAR asks once more, and stops asking after a while.
    var clearing by remember(core.tabs.viewed) { mutableStateOf(false) }
    LaunchedEffect(clearing) {
        if (clearing) {
            delay(4000)
            clearing = false
        }
    }
    Row(
        Modifier.fillMaxWidth().height(40.dp).padding(start = 16.dp, end = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(text, style = label(12.sp, Classic.label, 0.06.em))
        if (selecting) return@Row
        // Errors show in full below.
        val message = core.message?.takeIf { !it.error }
        val hint = if (list?.songs?.isNotEmpty() == true || queue?.rows?.isNotEmpty() == true) "HOLD A SONG TO SELECT" else ""
        Line(
            if (clearing) "" else message?.text ?: hint,
            label(12.sp, Classic.label, 0.02.em).copy(textAlign = TextAlign.End),
            Modifier.weight(1f),
        )
        // Held by another device, the queue only shows until taken over.
        if (queue?.heldBy != null) {
            ChromeButton(
                { core.apply(Action.TakeOverSpotify) },
                Modifier.height(32.dp),
                description = "Take the queue over from ${queue.heldBy}, paused",
            ) {
                BasicText("TAKE OVER", style = label(11.sp, Classic.chrome))
            }
            return@Row
        }
        // The queue's CLEAR stops it and empties it.
        val count = if (queue != null) queue.toCome.toInt() else list?.songs?.size ?: 0
        if (count > 0) {
            ChromeButton({
                if (clearing) {
                    if (queue != null) core.apply(Action.ClearSpotify) else list?.let { core.clear(it.id) }
                    clearing = false
                } else {
                    clearing = true
                }
            }, Modifier.height(32.dp), on = clearing) {
                val words = if (clearing) "CLEAR ${songs(count)}?" else "CLEAR"
                BasicText(words, style = label(11.sp, if (clearing) Classic.lcd else Classic.chrome))
            }
        }
    }
}

/** A song being dragged by its handle, and how far. */
private class Drag(val entry: ULong, val from: Int) {
    var offset by mutableFloatStateOf(0f)
}

@Composable
private fun Songs(core: Core, selected: List<ULong>, select: (Set<ULong>) -> Unit, modifier: Modifier) {
    val list = core.playlist
    val row = with(LocalDensity.current) { ROW.toPx() }
    val header = with(LocalDensity.current) { HEADER.toPx() }
    var drag by remember { mutableStateOf<Drag?>(null) }
    // The song a held one selects a range from: the last one picked on its own.
    var anchor by remember(list?.id) { mutableStateOf<ULong?>(null) }
    Box(modifier.fillMaxWidth().padding(horizontal = 8.dp).lcd().padding(2.dp)) {
        if (list == null || list.songs.isEmpty()) {
            BasicText(
                "No songs yet. Add some from Browse.",
                Modifier.padding(16.dp),
                body(color = Classic.dim),
            )
            return@Box
        }
        val songs = list.songs
        // Where each album's run starts, and how far down each song sits.
        val starts = songs.indices.filter { it == 0 || songs[it].track.album.uri != songs[it - 1].track.album.uri }
        val tops = FloatArray(songs.size)
        var y = 0f
        for (i in songs.indices) {
            if (i in starts) y += header
            tops[i] = y
            y += row
        }
        /** Where the dragged song would land: the song whose place is nearest. */
        fun target(d: Drag): Int {
            val at = tops[d.from] + d.offset
            return songs.indices.minBy { abs(tops[it] - at) }
        }
        LazyColumn(Modifier.fillMaxSize()) {
            songs.forEachIndexed { index, song ->
                if (index in starts) {
                    val end = starts.firstOrNull { it > index } ?: songs.size
                    val run = songs.subList(index, end).map { it.entry }
                    val all = selected.containsAll(run)
                    item(key = "album ${song.entry}") {
                        AlbumHeader(
                            song, all,
                            tap = {
                                when {
                                    selected.isEmpty() -> core.apply(Action.PlayEntry(list.id, song.entry))
                                    all -> select((selected - run.toSet()).toSet())
                                    else -> select((selected + run).toSet())
                                }
                            },
                            hold = {
                                anchor = song.entry
                                select((selected + run).toSet())
                            },
                        )
                    }
                }
                item(key = song.entry.toLong()) {
                    val chosen = song.entry in selected
                    val d = drag
                    // The dragged song follows the finger; the ones it passes take
                    // the place next to theirs, over any header between.
                    val shift = when {
                        d == null -> 0f
                        d.entry == song.entry -> d.offset
                        index in (d.from + 1)..target(d) -> tops[index - 1] - tops[index]
                        index in target(d) until d.from -> tops[index + 1] - tops[index]
                        else -> 0f
                    }
                    SongRow(
                        index + 1, song,
                        current = song.entry == list.cursor,
                        chosen = chosen,
                        selecting = selected.isNotEmpty(),
                        lifted = d?.entry == song.entry,
                        modifier = Modifier.zIndex(if (d?.entry == song.entry) 1f else 0f).graphicsLayer { translationY = shift },
                        tap = {
                            when {
                                selected.isEmpty() -> core.apply(Action.PlayEntry(list.id, song.entry))
                                chosen -> select((selected - song.entry).toSet())
                                else -> {
                                    anchor = song.entry
                                    select((selected + song.entry).toSet())
                                }
                            }
                        },
                        hold = {
                            val from = list.songs.indexOfFirst { it.entry == anchor }
                            if (anchor in selected && from >= 0) {
                                val range = list.songs.slice(minOf(from, index)..maxOf(from, index))
                                select((selected + range.map { it.entry }).toSet())
                            } else {
                                anchor = song.entry
                                select((selected + song.entry).toSet())
                            }
                        },
                        handle = Modifier.pointerInput(song.entry, index, selected, starts, songs.size) {
                            detectDragGestures(
                                onDragStart = { drag = Drag(song.entry, index) },
                                onDrag = { change, amount ->
                                    change.consume()
                                    drag?.let { it.offset += amount.y }
                                },
                                onDragEnd = {
                                    drag?.let { done ->
                                        val to = target(done)
                                        if (to != done.from) {
                                            // A selected song takes the rest of the selection along.
                                            val moved = if (chosen) selected else listOf(song.entry)
                                            val before = if (to < done.from) to else to + 1
                                            core.apply(Action.Move(moved, before.toUInt()))
                                        }
                                    }
                                    drag = null
                                },
                                onDragCancel = { drag = null },
                            )
                        },
                    )
                }
            }
        }
    }
}

/** Above an album's run of songs: its name and the first song's artist. */
@Composable
private fun AlbumHeader(song: Song, chosen: Boolean, tap: () -> Unit, hold: () -> Unit) {
    val album = song.track.album.name
    val artist = song.track.artists.firstOrNull()?.name
    Row(
        Modifier
            .fillMaxWidth()
            .height(HEADER)
            .background(if (chosen) Classic.selected else Classic.playingRow)
            .combinedClickable(onLongClick = hold, onClick = tap)
            .semantics { contentDescription = listOfNotNull(album, artist).joinToString(", by ") }
            .padding(start = 10.dp, end = 18.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.width(3.dp).height(14.dp).background(if (chosen) Classic.current else Classic.lcd))
        val text = buildAnnotatedString {
            withStyle(SpanStyle(color = if (chosen) Classic.current else Classic.lcd)) { append(album.uppercase()) }
            if (artist != null) {
                withStyle(SpanStyle(color = if (chosen) Color(0xFFD6DCFF) else Classic.dim)) {
                    append("  ·  ${artist.uppercase()}")
                }
            }
        }
        BasicText(
            text,
            Modifier.weight(1f),
            label(12.sp, spacing = 0.06.em),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/**
 * A song: its number (none in the queue), title, artists, Q mark and
 * length, and while selecting the handle that moves it (none where songs
 * can't move). A song played already is [faded].
 */
@Composable
private fun SongRow(
    number: Int?,
    song: Song,
    current: Boolean,
    chosen: Boolean,
    selecting: Boolean,
    lifted: Boolean,
    modifier: Modifier,
    tap: () -> Unit,
    hold: () -> Unit,
    handle: Modifier?,
    faded: Boolean = false,
) {
    val fg = when {
        current || chosen -> Classic.current
        faded -> Classic.dim
        else -> Classic.list
    }
    val sub = when {
        chosen -> Color(0xFFD6DCFF)
        current -> Classic.chrome
        else -> Classic.dim
    }
    val bg = when {
        chosen -> Classic.selected
        current -> Classic.playingRow
        lifted -> Classic.button
        else -> Color.Black
    }
    Row(
        modifier
            .fillMaxWidth()
            .height(ROW)
            .background(bg)
            .combinedClickable(onLongClick = hold, onClick = tap)
            .padding(start = if (number == null) 12.dp else 8.dp, end = if (selecting && handle != null) 4.dp else 18.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (number != null) {
            BasicText(
                "$number.", Modifier.width(30.dp),
                mono(color = if (current || chosen) Classic.current else Classic.dim).copy(textAlign = TextAlign.End),
            )
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Line(song.track.title, body(color = fg))
            Line(song.track.artists.joinToString(", ") { it.name }, body(13.sp, sub, FontWeight.Normal))
        }
        song.mark?.let { BasicText("Q$it", style = mono(13.sp, Classic.lcd)) }
        BasicText(clock(song.track.durationMs.toLong()), style = mono(color = fg))
        if (selecting && handle != null) {
            Box(
                handle
                    .width(40.dp)
                    .height(44.dp)
                    .semantics { contentDescription = "Drag to move" },
                contentAlignment = Alignment.Center,
            ) {
                Box(Modifier.size(18.dp).drawBehind { glyph(Glyph.Menu, sub) })
            }
        }
    }
}

/**
 * The Spotify tab's songs: what played, dimmed; the song playing; then
 * each run to come under its heading. Tapping a song to come plays the
 * queue here from it, taking it over from another device; tapping the song
 * playing plays it here. While another device holds the queue, the tab
 * only shows it: TAKE OVER, on the line above, brings it here paused.
 */
@Composable
private fun QueueSongs(core: Core, queue: SpotifyView, selected: List<ULong>, select: (Set<ULong>) -> Unit, modifier: Modifier) {
    val id = core.tabs.viewed
    val songs = queueSongs(queue)
    var anchor by remember { mutableStateOf<ULong?>(null) }
    val state = rememberLazyListState()
    // Opened on the song playing, below the ones played.
    val now = queue.rows.indexOfFirst { it is SpotifyRow.Song && it.played == Played.NOW }
    LaunchedEffect(now >= 0) {
        if (now > 0) state.scrollToItem(now - 1)
    }
    Column(modifier.fillMaxWidth().padding(horizontal = 8.dp).lcd().padding(2.dp)) {
        // Where it plays, always in sight: what can be done follows from it.
        Line(
            queue.whereItPlays,
            body(14.sp, Classic.lcd, FontWeight.SemiBold),
            Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
        )
        if (songs.none { it.played != Played.BEFORE }) {
            BasicText(
                if (queue.takesSongs) {
                    "Nothing to play. Add songs from Browse, or play something on another device."
                } else {
                    "Nothing to play. Play something on another device, or pick Spotless in Spotify's list of devices."
                },
                Modifier.padding(16.dp),
                body(color = Classic.dim),
            )
            return@Column
        }
        LazyColumn(Modifier.fillMaxSize(), state) {
            for (row in queue.rows) {
                when (row) {
                    is SpotifyRow.Heading -> item {
                        Box(
                            Modifier.fillMaxWidth().height(HEADER).background(Classic.playingRow).padding(start = 12.dp),
                            contentAlignment = Alignment.CenterStart,
                        ) {
                            Line(row.title.uppercase(), label(12.sp, Classic.lcd, 0.06.em))
                        }
                    }
                    is SpotifyRow.Song -> item(key = row.entry.toLong()) {
                        val chosen = row.entry in selected
                        val index = songs.indexOf(row)
                        SongRow(
                            null, Song(row.entry, row.track, null),
                            current = row.played == Played.NOW,
                            chosen = chosen,
                            selecting = selected.isNotEmpty(),
                            lifted = false,
                            modifier = Modifier,
                            tap = {
                                when {
                                    // What played before can't be played from.
                                    selected.isEmpty() -> if (row.played != Played.BEFORE) core.apply(Action.PlayEntry(id, row.entry))
                                    chosen -> select((selected - row.entry).toSet())
                                    else -> {
                                        anchor = row.entry
                                        select((selected + row.entry).toSet())
                                    }
                                }
                            },
                            hold = {
                                val from = songs.indexOfFirst { it.entry == anchor }
                                if (anchor in selected && from >= 0) {
                                    val range = songs.slice(minOf(from, index)..maxOf(from, index))
                                    select((selected + range.map { it.entry }).toSet())
                                } else {
                                    anchor = row.entry
                                    select((selected + row.entry).toSet())
                                }
                            },
                            handle = null,
                            faded = row.played == Played.BEFORE,
                        )
                    }
                }
            }
        }
    }
}
