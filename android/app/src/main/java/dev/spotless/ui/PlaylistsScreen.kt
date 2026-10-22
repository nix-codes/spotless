package dev.spotless.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectDragGestures
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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.BasicTextField
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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import androidx.compose.ui.zIndex
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.Song
import kotlin.math.roundToInt

private val ROW = 52.dp

/**
 * The main screen: the playlist tabs, the viewed playlist, and the player.
 * Holding a song selects it, for Q NEXT, removing and moving; holding another
 * then selects every song from the last one picked up to it.
 */
@Composable
fun PlaylistsScreen(core: Core, openSettings: () -> Unit) {
    // Entries of the viewed playlist; ones since removed drop out.
    var picked by remember(core.tabs.viewed) { mutableStateOf(emptySet<ULong>()) }
    val entries = core.playlist?.songs?.map { it.entry }.orEmpty()
    val selected = entries.filter { it in picked }
    val selecting = selected.isNotEmpty()
    var editing by remember { mutableStateOf<ULong?>(null) }
    BackHandler(enabled = selecting) { picked = emptySet() }
    BackHandler(enabled = editing != null) { editing = null }
    Column(Modifier.fillMaxSize().background(Classic.frame).safeDrawingPadding()) {
        if (selecting) {
            SelectBar(core, selected, { picked = entries.toSet() }) { picked = emptySet() }
        } else {
            TopBar(core, openSettings)
        }
        Tabs(core) { editing = it }
        editing?.let { TabEditor(core, it) { editing = null } }
        Summary(core, selecting)
        Songs(core, selected, { picked = it }, Modifier.weight(1f))
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
        BasicText("P L A Y L I S T S", Modifier.weight(1f), label(spacing = 0.3.em))
        GlyphButton(Glyph.Search, "Browse", { core.apply(Action.OpenBrowse) }, Modifier.width(48.dp))
        GlyphButton(Glyph.Menu, "Settings", openSettings, Modifier.width(48.dp))
    }
}

/** In place of the top bar while songs are selected. */
@Composable
private fun SelectBar(core: Core, selected: List<ULong>, all: () -> Unit, done: () -> Unit) {
    val list = core.playlist ?: return
    Row(
        Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GlyphButton(Glyph.Close, "Stop selecting", done, Modifier.width(48.dp))
        BasicText(
            "${selected.size} selected",
            Modifier.weight(1f).padding(start = 4.dp),
            body(15.sp, Classic.current, FontWeight.SemiBold),
        )
        TextButton("ALL", all, enabled = selected.size < list.songs.size)
        TextButton("Q NEXT", { core.apply(Action.ToggleMarks(selected)) })
        GlyphButton(Glyph.Trash, "Remove from the playlist", {
            core.apply(Action.Remove(list.id, selected))
            done()
        }, Modifier.width(48.dp))
    }
}

/** The playlists' tabs; holding one opens its editor. */
@Composable
private fun Tabs(core: Core, edit: (ULong) -> Unit) {
    LazyRow(
        Modifier.fillMaxWidth().height(44.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        contentPadding = PaddingValues(horizontal = 8.dp),
    ) {
        items(core.tabs.tabs, key = { it.id.toLong() }) { tab ->
            val on = tab.id == core.tabs.viewed
            ChromeButton(
                { core.apply(Action.SelectPlaylist(tab.id)) },
                Modifier.fillMaxHeight(),
                on = on,
                onLongClick = { edit(tab.id) },
            ) {
                BasicText(tab.name, style = body(14.sp, if (on) Classic.lcd else Classic.chrome))
            }
        }
        item {
            ChromeButton({ core.apply(Action.NewPlaylist) }, Modifier.fillMaxHeight(), description = "New playlist") {
                BasicText("+", style = label(20.sp))
            }
        }
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

@Composable
private fun Summary(core: Core, selecting: Boolean) {
    val list = core.playlist
    val text = when {
        selecting -> "TAP ADD/DROP · HOLD RANGE · DRAG ≡ MOVE"
        list != null -> "${songs(list.songs.size)} · ${clock(list.durationMs.toLong())}"
        else -> ""
    }
    // CLEAR asks once more, and stops asking after a while.
    var clearing by remember(list?.id) { mutableStateOf(false) }
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
        val message = core.message
        val hint = if (list?.songs?.isNotEmpty() == true) "HOLD A SONG TO SELECT" else ""
        Line(
            if (clearing) "" else message?.text ?: hint,
            label(12.sp, if (message?.error == true) Color(0xFFFFB000) else Classic.label, 0.02.em)
                .copy(textAlign = TextAlign.End),
            Modifier.weight(1f),
        )
        if (list != null && list.songs.isNotEmpty()) {
            ChromeButton({
                if (clearing) {
                    core.clear(list.id)
                    clearing = false
                } else {
                    clearing = true
                }
            }, Modifier.height(32.dp), on = clearing) {
                val words = if (clearing) "CLEAR ${songs(list.songs.size)}?" else "CLEAR"
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
        val last = list.songs.lastIndex
        /** Where the dragged song would land. */
        fun target(d: Drag) = (d.from + (d.offset / row).roundToInt()).coerceIn(0, last)
        LazyColumn(Modifier.fillMaxSize()) {
            itemsIndexed(list.songs, key = { _, song -> song.entry.toLong() }) { index, song ->
                val chosen = song.entry in selected
                val d = drag
                // The dragged song follows the finger; the ones it passes make way.
                val shift = when {
                    d == null -> 0f
                    d.entry == song.entry -> d.offset
                    index in (d.from + 1)..target(d) -> -row
                    index in target(d) until d.from -> row
                    else -> 0f
                }
                SongRow(
                    index, song,
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
                    handle = Modifier.pointerInput(song.entry, index, selected, last) {
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

@Composable
private fun SongRow(
    index: Int,
    song: Song,
    current: Boolean,
    chosen: Boolean,
    selecting: Boolean,
    lifted: Boolean,
    modifier: Modifier,
    tap: () -> Unit,
    hold: () -> Unit,
    handle: Modifier,
) {
    val fg = if (current || chosen) Classic.current else Classic.list
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
            .padding(start = 8.dp, end = if (selecting) 4.dp else 18.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(
            "${index + 1}.", Modifier.width(30.dp),
            mono(color = if (current || chosen) Classic.current else Classic.dim).copy(textAlign = TextAlign.End),
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Line(song.track.title, body(color = fg))
            Line(song.track.artists.joinToString(", ") { it.name }, body(13.sp, sub, FontWeight.Normal))
        }
        song.mark?.let { BasicText("Q$it", style = mono(13.sp, Classic.lcd)) }
        BasicText(clock(song.track.durationMs.toLong()), style = mono(color = fg))
        if (selecting) {
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
