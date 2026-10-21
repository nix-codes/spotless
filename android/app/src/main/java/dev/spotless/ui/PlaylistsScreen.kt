package dev.spotless.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.Song

/** The main screen: the playlist tabs, the viewed playlist, and the player. */
@Composable
fun PlaylistsScreen(core: Core) {
    Column(Modifier.fillMaxSize().background(Classic.frame).safeDrawingPadding()) {
        TopBar(core)
        Tabs(core)
        Summary(core)
        Songs(core, Modifier.weight(1f))
        PlayerPanel(core)
    }
}

@Composable
private fun TopBar(core: Core) {
    Row(
        Modifier.fillMaxWidth().height(56.dp).padding(start = 16.dp, end = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText("P L A Y L I S T S", Modifier.weight(1f), label(spacing = 0.3.em))
        GlyphButton(Glyph.Search, "Browse", { core.apply(Action.OpenBrowse) }, Modifier.width(48.dp))
    }
}

@Composable
private fun Tabs(core: Core) {
    LazyRow(
        Modifier.fillMaxWidth().height(44.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        contentPadding = PaddingValues(horizontal = 8.dp),
    ) {
        items(core.tabs.tabs, key = { it.id.toLong() }) { tab ->
            val on = tab.id == core.tabs.viewed
            ChromeButton({ core.apply(Action.SelectPlaylist(tab.id)) }, Modifier.fillMaxHeight(), on = on) {
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

@Composable
private fun Summary(core: Core) {
    val list = core.playlist
    val text = list?.let { "${songs(it.songs.size)} · ${clock(it.durationMs.toLong())}" } ?: ""
    Row(
        Modifier.fillMaxWidth().height(32.dp).padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(text, style = label(12.sp, Classic.label, 0.06.em))
        core.message?.let {
            Line(
                it.text, label(12.sp, if (it.error) Color(0xFFFFB000) else Classic.label, 0.02.em),
                Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun Songs(core: Core, modifier: Modifier) {
    val list = core.playlist
    Box(modifier.fillMaxWidth().padding(horizontal = 8.dp).lcd().padding(2.dp)) {
        if (list == null || list.songs.isEmpty()) {
            BasicText(
                "No songs yet. Add some from Browse.",
                Modifier.padding(16.dp),
                body(color = Classic.dim),
            )
            return@Box
        }
        LazyColumn(Modifier.fillMaxSize()) {
            itemsIndexed(list.songs, key = { _, song -> song.entry.toLong() }) { index, song ->
                SongRow(index, song, song.entry == list.cursor) {
                    core.apply(Action.PlayEntry(list.id, song.entry))
                }
            }
        }
    }
}

@Composable
private fun SongRow(index: Int, song: Song, current: Boolean, play: () -> Unit) {
    val fg = if (current) Classic.current else Classic.list
    val sub = if (current) Classic.chrome else Classic.dim
    Row(
        Modifier
            .fillMaxWidth()
            .height(52.dp)
            .background(if (current) Classic.playingRow else Color.Transparent)
            .clickable(onClick = play)
            .padding(start = 8.dp, end = 18.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(
            "${index + 1}.", Modifier.width(30.dp),
            mono(color = if (current) Classic.current else Classic.dim).copy(textAlign = TextAlign.End),
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Line(song.track.title, body(color = fg))
            Line(song.track.artists.joinToString(", ") { it.name }, body(13.sp, sub, FontWeight.Normal))
        }
        song.mark?.let { BasicText("Q$it", style = mono(13.sp, Classic.lcd)) }
        BasicText(clock(song.track.durationMs.toLong()), style = mono(color = fg))
    }
}
