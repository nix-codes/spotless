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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyListState
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
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import kotlinx.coroutines.launch
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import kotlinx.coroutines.flow.first
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.draw.drawWithContent
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
import dev.spotless.core.Header
import dev.spotless.core.LastFm
import dev.spotless.core.Played
import dev.spotless.core.Song
import dev.spotless.core.SpotifyRow
import dev.spotless.core.SpotifyView
import dev.spotless.core.TabSync
import dev.spotless.core.Track
import dev.spotless.core.Session
import kotlin.math.abs
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Spacer
import androidx.compose.runtime.produceState
import androidx.compose.runtime.withFrameMillis
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Path
import dev.spotless.core.Status
import kotlin.math.sin

private val ROW = 52.dp
private val HEADER = 30.dp

/**
 * The main screen: the playlist tabs, the viewed playlist, and the player.
 * Holding a song selects it, for Q NEXT, copying, removing and moving;
 * holding another then selects every song from the last one picked up to
 * it. Two or more songs in a row from one album sit under a header with
 * its name; a line sets off the lone songs that follow.
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
    var sending by remember(core.tabs.viewed) { mutableStateOf<Send?>(null) }
    // COPY's SPOTIFY…: the picker in place of the tabs to copy to.
    var toSpotify by remember(core.tabs.viewed) { mutableStateOf(false) }
    var editing by remember { mutableStateOf<ULong?>(null) }
    var adding by remember { mutableStateOf(false) }
    val done = {
        picked = emptySet()
        sending = null
        toSpotify = false
    }
    BackHandler(enabled = selecting) { done() }
    BackHandler(enabled = editing != null) { editing = null }
    BackHandler(enabled = adding) { adding = false }
    Column(Modifier.fillMaxSize().background(Classic.frame).safeDrawingPadding()) {
        if (selecting) {
            val movable = queue == null || movable(queue, selected)
            SelectBar(core, queue, selected, { picked = entries.toSet() }, sending, movable, { sending = it.takeIf { it != sending } }, done)
            val tracks = selectedTracks(core, queue, selected)
            if (toSpotify) {
                val name = core.tabs.tabs.firstOrNull { it.id == core.tabs.viewed }?.name.orEmpty()
                SpotifyPicker(core, tracks, name, saving = false, close = { toSpotify = false }, done = done)
            } else {
                sending?.takeIf { it == Send.COPY || movable }?.let {
                    SendBar(core, it, selected, tracks, { toSpotify = true }, done)
                }
            }
        } else {
            TopBar(core, openSettings)
        }
        val naming = remember(editing) {
            editing?.let { id -> core.tabs.tabs.firstOrNull { it.id == id && !it.spotify }?.let { NameEdit(id, it.name) } }
        }
        // A tab held is shown too: the one pressed in is the one edited.
        Tabs(core, naming, {
            editing = it
            core.apply(Action.SelectPlaylist(it))
        }, { adding = !adding }, { if (it != editing) editing = null })
        editing?.let { TabEditor(core, it, naming) { editing = null } }
        if (adding && core.settings.spotifyTabClosed) AddBar(core) { adding = false }
        Summary(core, queue, selected)
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

/**
 * Of the queue, only songs to come can move to a playlist, and only while
 * it takes changes: Spotless has it, or no device does.
 */
private fun movable(queue: SpotifyView, selected: List<ULong>) =
    queue.takesSongs && queueSongs(queue).filter { it.entry in selected }.all { it.played == Played.NEXT }

/** What COPY and MOVE do with the selected songs. */
private enum class Send { COPY, MOVE }

/** In place of the top bar while songs are selected. */
@Composable
private fun SelectBar(
    core: Core,
    queue: SpotifyView?,
    selected: List<ULong>,
    all: () -> Unit,
    sending: Send?,
    movable: Boolean,
    send: (Send) -> Unit,
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
        // How many are selected shows below the tabs: the buttons need the room.
        Box(Modifier.weight(1f))
        TextButton("ALL", all, enabled = selected.size < count)
        if (queue == null) TextButton("Q NEXT", { core.apply(Action.ToggleMarks(selected)) })
        TextButton("COPY", { send(Send.COPY) }, on = sending == Send.COPY)
        TextButton("MOVE", { send(Send.MOVE) }, on = sending == Send.MOVE && movable, enabled = movable)
        GlyphButton(Glyph.Trash, if (queue != null) "Remove from the queue" else "Remove from the playlist", {
            core.apply(Action.Remove(id, removable))
            done()
        }, Modifier.width(48.dp), enabled = removable.isNotEmpty())
    }
}

/**
 * While Last.fm is connected, LOVE beside the count of songs selected (the
 * selection bar has no room): UNLOVE while they're all loved there.
 */
@Composable
private fun Love(core: Core, queue: SpotifyView?, selected: List<ULong>) {
    val songs = if (queue != null) {
        queueSongs(queue).filter { it.entry in selected }.map { it.track to it.loved }
    } else {
        core.playlist?.songs?.filter { it.entry in selected }?.map { it.track to it.loved }.orEmpty()
    }
    val all = songs.all { it.second }
    ChromeButton({
        songs
            .filter { it.second == all }
            .map { (track, _) -> track.artists.firstOrNull()?.name.orEmpty() to track.title }
            // Each song once, however often it's in the tab.
            .distinctBy { (artist, title) -> artist.lowercase() to title.lowercase() }
            .forEach { (artist, title) -> core.apply(Action.SetLoved(artist, title, !all)) }
    }, Modifier.height(32.dp), description = if (all) "Unlove on Last.fm" else "Love on Last.fm") {
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(12.dp).drawBehind { glyph(if (all) Glyph.Unloved else Glyph.Heart, Classic.chrome) })
            BasicText(if (all) "UNLOVE" else "LOVE", style = label(11.sp, Classic.chrome))
        }
    }
}

/**
 * COPY's and MOVE's choice of tab: the selected songs go to the end of the
 * one tapped, or make a new playlist, which shows.
 */
@Composable
private fun SendBar(core: Core, send: Send, selected: List<ULong>, tracks: List<Track>, toSpotify: () -> Unit, done: () -> Unit) {
    val queue = core.spotify
    val from = core.tabs.viewed
    Column(
        Modifier.fillMaxWidth().padding(8.dp).background(Classic.frame).bevel().padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        BasicText(if (send == Send.COPY) "COPY TO" else "MOVE TO", style = label(12.sp, Classic.label, 0.06.em))
        LazyRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            // First, where it can always be seen.
            item(key = "new") {
                TextButton("+ NEW", {
                    core.apply(Action.NewPlaylistWith(tracks))
                    if (send == Send.MOVE) core.apply(Action.Remove(from, selected))
                    done()
                }, Modifier.height(44.dp))
            }
            // A copy only: songs on Spotify stay in the tab too.
            if (send == Send.COPY) {
                item(key = "spotify") { TextButton("SPOTIFY…", toSpotify, Modifier.height(44.dp)) }
                // Their links, to another app.
                item(key = "share") {
                    TextButton("SHARE…", {
                        core.share(tracks.map { it.uri })
                        done()
                    }, Modifier.height(44.dp))
                }
            }
            items(core.tabs.tabs.filter { it.id != from }, key = { it.id.toLong() }) { tab ->
                // Spotify takes songs for the queue only from the device playing it.
                TextButton(tab.name, {
                    core.apply(if (send == Send.COPY) Action.CopyTo(tab.id, tracks) else Action.MoveTo(from, tab.id, selected))
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
 * The playlists' tabs. Holding one opens its editor, its name typed in the
 * tab itself; holding it and sliding moves it, the others making room.
 */
@Composable
private fun Tabs(core: Core, naming: NameEdit?, edit: (ULong) -> Unit, add: () -> Unit, tapped: (ULong?) -> Unit) {
    val tabs = core.tabs.tabs
    val state = rememberLazyListState()
    var drag by remember { mutableStateOf<TabDrag?>(null) }
    val gap = with(LocalDensity.current) { 4.dp.toPx() }
    val haptics = LocalHapticFeedback.current
    // The viewed tab in sight, or the one held: there already on opening,
    // glided to after.
    val viewed = tabs.indexOfFirst { it.id == (naming?.id ?: core.tabs.viewed) }
    var opened by remember { mutableStateOf(false) }
    LaunchedEffect(viewed) {
        if (viewed < 0) return@LaunchedEffect
        // Laid out first, so a tab already in sight stays put.
        snapshotFlow { state.layoutInfo.totalItemsCount }.first { it > 0 }
        val info = state.layoutInfo
        val item = info.visibleItemsInfo.firstOrNull { it.index == viewed }
        val seen = item != null && item.offset >= info.viewportStartOffset && item.offset + item.size <= info.viewportEndOffset
        if (!seen) if (opened) state.animateScrollToItem(viewed) else state.scrollToItem(viewed)
        opened = true
    }
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
                {
                    // Another tab than the one held: its editor closes.
                    tapped(tab.id)
                    core.apply(Action.SelectPlaylist(tab.id))
                },
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
                Row(verticalAlignment = Alignment.CenterVertically) {
                    // The playing tab's mark: bars while a song plays, ▶ paused.
                    if (tab.id == core.player.playlist) {
                        if (core.player.status == Status.PLAYING) DancingBars() else PlayMark()
                        Spacer(Modifier.width(6.dp))
                    }
                    if (naming?.id == tab.id) {
                        NameField(core, naming)
                    } else {
                        BasicText(tab.name, style = body(14.sp, if (on) Classic.lcd else Classic.chrome))
                    }
                    // A synced one's mark, lit while it's the same as elsewhere.
                    tab.sync?.let { sync ->
                        val color = when (sync) {
                            TabSync.UpToDate -> if (on) Classic.lcd else Classic.chrome
                            TabSync.Waiting -> Classic.label
                            is TabSync.Failed -> Classic.warning
                        }
                        Spacer(Modifier.width(6.dp))
                        Box(Modifier.size(14.dp).drawBehind { glyph(Glyph.Sync, color) })
                    }
                }
            }
        }
        item {
            // With the Spotify tab closed, + opens it too.
            val closed = core.settings.spotifyTabClosed
            ChromeButton(
                {
                    tapped(null)
                    if (closed) add() else core.apply(Action.NewPlaylist)
                },
                Modifier.fillMaxHeight(),
                description = if (closed) "New playlist or the Spotify tab" else "New playlist",
            ) {
                BasicText("+", style = label(20.sp))
            }
        }
    }
}

/** Three bars bouncing like a level meter, as the desktop's playing tab has. */
@Composable
private fun DancingBars() {
    val time by produceState(0f) {
        val start = withFrameMillis { it }
        while (true) withFrameMillis { value = (it - start) / 1000f }
    }
    Canvas(Modifier.size(9.dp, 10.dp)) {
        val unit = size.width / 9f
        listOf(7f to 0f, 9.5f to 1.3f, 6f to 2.4f).forEachIndexed { n, (speed, phase) ->
            val height = size.height * (2f + 7f * abs(sin(time * speed + phase))) / 9f
            drawRect(
                Classic.lcd,
                topLeft = Offset(n * 3.5f * unit, size.height - height),
                size = Size(2.5f * unit, height),
            )
        }
    }
}

/** The playing tab's mark while its song is paused. */
@Composable
private fun PlayMark() {
    Canvas(Modifier.size(9.dp, 10.dp)) {
        drawPath(
            Path().apply {
                moveTo(size.width * 0.15f, 0f)
                lineTo(size.width * 0.95f, size.height / 2)
                lineTo(size.width * 0.15f, size.height)
                close()
            },
            Classic.lcd,
        )
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

/** The held tab's name as typed: in the tab, kept by the editor. */
private class NameEdit(val id: ULong, name: String) {
    var name by mutableStateOf(name)
    val field = FocusRequester()

    /** The name typed, kept; left empty, it's the old name again. */
    fun keep(core: Core) {
        val now = core.tabs.tabs.firstOrNull { it.id == id }?.name ?: return
        if (name.isBlank()) {
            name = now
        } else if (name.trim() != now) {
            core.apply(Action.RenamePlaylist(id, name.trim()))
        }
    }
}

/** The held tab's name, typed in place; the keyboard comes on a tap. */
@Composable
private fun NameField(core: Core, naming: NameEdit) {
    val focus = LocalFocusManager.current
    BasicTextField(
        naming.name,
        { naming.name = it },
        Modifier
            .widthIn(min = 32.dp, max = 220.dp)
            .focusRequester(naming.field)
            // Underlined: it can be typed in.
            .drawBehind {
                drawLine(Classic.lcd, Offset(0f, size.height), Offset(size.width, size.height), 1.dp.toPx())
            }
            .semantics { contentDescription = "Playlist name" },
        textStyle = body(14.sp, Classic.lcd),
        singleLine = true,
        cursorBrush = SolidColor(Classic.lcd),
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = {
            naming.keep(core)
            focus.clearFocus()
        }),
    )
}

/**
 * A held tab's syncing, exporting and closing or deleting, and ? for what
 * they do. Its name is typed in the tab.
 */
@Composable
private fun TabEditor(core: Core, id: ULong, naming: NameEdit?, done: () -> Unit) {
    val tabs = core.tabs.tabs
    val at = tabs.indexOfFirst { it.id == id }
    if (at < 0) {
        // Deleted meanwhile.
        LaunchedEffect(id) { done() }
        return
    }
    val tab = tabs[at]
    if (tab.spotify || naming == null) return SpotifyTabEditor(core, id, at, done)
    // Deleting asks once more, on the same button.
    var deleting by remember(id) { mutableStateOf(false) }
    var saving by remember(id) { mutableStateOf(false) }
    var help by remember(id) { mutableStateOf(false) }
    val focus = LocalFocusManager.current
    // The name typed is kept before whatever is tapped next.
    val keep = {
        naming.keep(core)
        focus.clearFocus()
    }
    // Closed some other way, or another tab held: still kept.
    DisposableEffect(id) { onDispose { keep() } }
    if (saving) {
        return SpotifyPicker(core, core.tracks(id), tab.name, saving = true, close = { saving = false }, done = done)
    }
    val synced = tab.sync != null
    val signedIn = core.session is Session.Ready
    Column(
        Modifier.fillMaxWidth().padding(8.dp).background(Classic.frame).bevel().padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (help) {
            TabHelp {
                help = false
            }
            return@Column
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            // Pressed in, with the tab's ⇅, while it's synced.
            ChromeButton({
                keep()
                core.apply(Action.SetSync(id, !synced))
            }, Modifier.width(112.dp), on = synced, enabled = signedIn, description = if (synced) "Synced; tap to stop" else "Sync") {
                val color = when {
                    !signedIn -> Classic.label
                    synced -> Classic.lcd
                    else -> Classic.chrome
                }
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (synced) Box(Modifier.size(14.dp).drawBehind { glyph(Glyph.Sync, color) })
                    BasicText(if (synced) "SYNCED" else "SYNC", style = label(color = color))
                }
            }
            // Its songs, copied once to a new playlist on Spotify.
            TextButton("EXPORT TO SPOTIFY…", {
                keep()
                saving = true
            }, Modifier.weight(1f), enabled = tab.songs > 0u)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            if (!deleting) {
                TextButton("?", {
                    keep()
                    help = true
                }, Modifier.width(48.dp))
            }
            Box(Modifier.weight(1f))
            // A synced one closes only: it stays on Spotify, and YOUR
            // SPOTIFY in Browse opens it again or deletes it. The last one
            // leaves an empty Default.
            if (synced) {
                TextButton("CLOSE TAB", {
                    keep()
                    core.apply(Action.ClosePlaylist(id))
                    done()
                })
            } else {
                val delete = {
                    // An empty playlist has nothing to lose: no second ask.
                    if (deleting || tab.songs == 0u) {
                        core.apply(Action.ClosePlaylist(id))
                        done()
                    } else {
                        keep()
                        deleting = true
                    }
                }
                if (deleting) {
                    TextButton("DELETE ${songs(tab.songs.toInt())}?", delete, lit = true)
                } else {
                    GlyphButton(Glyph.Trash, "Delete the playlist", delete, Modifier.width(48.dp))
                }
            }
            GlyphButton(Glyph.Close, "Done", {
                keep()
                done()
            }, Modifier.width(48.dp))
        }
        // Only what needs a look: changes not yet sent, or refused.
        when (val sync = tab.sync) {
            TabSync.Waiting -> "Changes wait to be sent."
            is TabSync.Failed -> "Spotify refused a change: ${sync.reason}"
            else -> null
        }?.let {
            BasicText(it, style = body(13.sp, if (tab.sync is TabSync.Failed) Classic.warning else Classic.label, FontWeight.Normal))
        }
        // Not synced: the Spotless folder has its name. Renamed, it's synced.
        if (core.tabs.syncNameTaken == id) {
            BasicText(
                "A synced playlist called “${tab.name}” is already on Spotify. Rename this one, " +
                    "or replace that one: your other devices keep it, no longer synced.",
                style = body(13.sp, Classic.warning, FontWeight.Normal),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton("RENAME", { naming.field.requestFocus() }, Modifier.weight(1f))
                TextButton("REPLACE IT", {
                    keep()
                    core.apply(Action.ReplaceSynced(id))
                }, Modifier.weight(1f))
                TextButton("CANCEL", {
                    keep()
                    core.apply(Action.DismissSyncNameTaken)
                }, Modifier.weight(1f))
            }
        }
    }
}

/** What the tab editor's buttons do, in their place until OK. */
@Composable
private fun TabHelp(ok: () -> Unit) {
    @Composable
    fun Line(what: @Composable () -> Unit, does: String) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Box(Modifier.width(72.dp)) { what() }
            BasicText(does, Modifier.weight(1f), body(13.sp, Classic.chrome, FontWeight.Normal))
        }
    }
    @Composable
    fun Word(text: String) = BasicText(text, style = label(color = Classic.lcd))
    @Composable
    fun Mark(glyph: Glyph) = Box(Modifier.size(16.dp).drawBehind { glyph(glyph, Classic.lcd) })
    Line({ Word("NAME") }, "Tap the name in the tab to rename it.")
    Line({ Word("⇅ SYNC") }, "Keeps it the same on your other devices: its songs and where it's up to. Tap again to stop; it stays only here.")
    Line({ Word("EXPORT…") }, "Makes a new playlist on Spotify with its songs, once. Changing the tab later doesn't change it.")
    Line({ Mark(Glyph.Trash) }, "Deletes the playlist. A synced one has CLOSE TAB instead: it stays on Spotify, and Browse's YOUR SPOTIFY opens it again.")
    Line({ Word("MOVE") }, "Hold a tab and slide it.")
    Row {
        Box(Modifier.weight(1f))
        TextButton("OK", ok, Modifier.width(72.dp))
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
private fun Summary(core: Core, queue: SpotifyView?, selected: List<ULong>) {
    val list = core.playlist
    val tab = core.tabs.tabs.firstOrNull { it.id == core.tabs.viewed }
    val selecting = selected.isNotEmpty()
    val text = when {
        selecting -> "${selected.size} SELECTED"
        queue != null && tab != null -> "${songs(queue.toCome.toInt())} TO COME · ${clock(tab.durationMs.toLong())}"
        list != null -> "${songs(list.songs.size)} · ${clock(list.durationMs.toLong())}" +
            // Counted up, as the time left of the song is.
            (list.leftMs?.let { " · ${clock((it.toLong() + 999) / 1000 * 1000)} LEFT" } ?: "")
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
        BasicText(text, style = label(12.sp, if (selecting) Classic.current else Classic.label, 0.06.em))
        if (selecting) {
            // LOVE takes the room of the drag's hint.
            val love = core.settings.lastfm is LastFm.Connected
            Line(
                if (queue != null || love) "TAP ADD/DROP · HOLD RANGE" else "TAP ADD/DROP · HOLD RANGE · DRAG ≡ REORDERS",
                label(12.sp, Classic.label, 0.02.em).copy(textAlign = TextAlign.End),
                Modifier.weight(1f),
            )
            if (love) Love(core, queue, selected)
            return@Row
        }
        // Errors show in full below.
        val message = core.message?.takeIf { !it.error }
        // Stop after this song outranks the latest news: Stop, pressed in, says so too.
        val stopping = if (core.player.stopAfter) "STOPS AFTER THIS SONG" else null
        Line(
            if (clearing) "" else stopping ?: message?.text ?: "",
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
            if (clearing) {
                ChromeButton({
                    if (queue != null) core.apply(Action.ClearSpotify) else list?.let { core.clear(it.id) }
                    clearing = false
                }, Modifier.height(32.dp), on = true) {
                    BasicText("CLEAR ${songs(count)}?", style = label(11.sp, Classic.lcd))
                }
            } else {
                GlyphButton(Glyph.Clear, "Clear", { clearing = true }, Modifier.height(32.dp).width(44.dp))
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
        // Where each header's run starts, and how far down each song sits.
        val starts = songs.indices.filter { songs[it].header != null }
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
        // Opened on the cursor's song, with the row above it in sight.
        fun opening(): Int {
            val at = songs.indexOfFirst { it.entry == list.cursor }
            return if (at > 0) at + starts.count { it <= at } - 1 else 0
        }
        val state = remember(list.id) { LazyListState(opening()) }
        // A tap on the player's song glides it there again: in a scope of
        // its own, or clearing the jump would end this effect, and the
        // glide with it.
        val gliding = rememberCoroutineScope()
        LaunchedEffect(core.jump, list.id) {
            if (core.jump == list.id) {
                core.jump = null
                gliding.launch { state.glideTo(opening()) }
            }
        }
        ScrollingList(state, Modifier.fillMaxSize()) {
            songs.forEachIndexed { index, song ->
                if (index in starts) {
                    val header = song.header!!
                    val run = songs.subList(index, index + header.songs.toInt()).map { it.entry }
                    val all = selected.containsAll(run)
                    item(key = "header ${song.entry}") {
                        RunHeader(
                            header, all,
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
                        modifier = Modifier.zIndex(if (d?.entry == song.entry) 1f else 0f).graphicsLayer { translationY = shift }
                            .then(if (song.setOff) Modifier.setOff() else Modifier),
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

/** The line over a lone song that follows an album's run. */
private fun Modifier.setOff() = drawWithContent {
    drawContent()
    drawLine(Classic.lcd.copy(alpha = 0.3f), Offset(0f, 0.5f), Offset(size.width, 0.5f), 1.dp.toPx())
}

/**
 * Above a run of songs from one album or one source: its name, and what
 * it is or whose (an album's artist, "Spotify playlist").
 */
@Composable
private fun RunHeader(header: Header, chosen: Boolean, tap: () -> Unit, hold: () -> Unit) {
    val name = header.name
    val detail = header.detail
    Row(
        Modifier
            .fillMaxWidth()
            .height(HEADER)
            .background(if (chosen) Classic.selected else Classic.playingRow)
            .combinedClickable(onLongClick = hold, onClick = tap)
            .semantics { contentDescription = listOfNotNull(name, detail).joinToString(", ") }
            .padding(start = 10.dp, end = 18.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.width(3.dp).height(14.dp).background(if (chosen) Classic.current else Classic.lcd))
        val text = buildAnnotatedString {
            withStyle(SpanStyle(color = if (chosen) Classic.current else Classic.lcd)) { append(name.uppercase()) }
            if (detail != null) {
                withStyle(SpanStyle(color = if (chosen) Color(0xFFD6DCFF) else Classic.dim)) {
                    append("  ·  ${detail.uppercase()}")
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
 * A song: its number (none in the queue), title, artists, Q mark, a
 * heart if it's loved on Last.fm, and length, and while selecting the handle that moves it (none where songs
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
        if (song.loved) Box(Modifier.size(13.dp).drawBehind { glyph(Glyph.Heart, fg) })
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
    // A tap on the player's song glides it there again, in a scope of its
    // own (see Songs).
    val gliding = rememberCoroutineScope()
    LaunchedEffect(core.jump, id) {
        if (core.jump == id) {
            core.jump = null
            gliding.launch { state.glideTo((now - 1).coerceAtLeast(0)) }
        }
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
        ScrollingList(state, Modifier.fillMaxSize()) {
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
                            null, Song(row.entry, row.track, null, null, false, row.loved),
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
