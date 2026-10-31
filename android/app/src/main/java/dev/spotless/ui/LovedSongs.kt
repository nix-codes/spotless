package dev.spotless.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.LovedSong
import dev.spotless.core.LovedSongs
import java.text.Normalizer
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Locale

/**
 * Adding loved songs to a playlist. Off while too many aren't found on
 * Spotify, their names there being spelt otherwise.
 */
private const val ADDING = false

/** How LAST.FM's songs are sorted. */
enum class LovedOrder { ARTIST, TITLE, DATE }

/** A title's letters and digits, lowercase and without accents, as the core compares names. */
private fun nameKey(name: String) =
    Normalizer.normalize(name, Normalizer.Form.NFKD).filter { it.isLetterOrDigit() }.lowercase()

/**
 * Browse's LAST.FM, while Last.fm is connected and search set up: the
 * songs loved there, by artist, title or when. Last.fm knows them only by
 * artist and title, so Spotless finds each on Spotify first, and says
 * which it couldn't. A tap plays a song now, + adds it (with [ADDING]);
 * holding one selects it, holding another then selects the ones between,
 * to add or unlove together.
 */
@Composable
fun LovedSongs(core: Core, loved: LovedSongs, modifier: Modifier) {
    val order = core.lovedOrder
    // The core keeps them by artist, then title.
    val songs = remember(loved.songs, order) {
        when (order) {
            LovedOrder.ARTIST -> loved.songs
            LovedOrder.TITLE -> loved.songs.sortedBy { nameKey(it.title) }
            LovedOrder.DATE -> loved.songs.sortedByDescending { it.date }
        }
    }
    // The songs' indices; the selection goes once the songs change.
    var picked by remember(songs) { mutableStateOf(emptySet<Int>()) }
    var anchor by remember(songs) { mutableStateOf<Int?>(null) }
    val selected = picked.sorted()
    val done = { picked = emptySet() }
    BackHandler(enabled = selected.isNotEmpty()) { done() }
    Column(modifier) {
        if (selected.isNotEmpty()) {
            SelectBar(core, songs, selected, { picked = songs.indices.toSet() }, done)
        } else {
            Row(
                Modifier.fillMaxWidth().height(48.dp).padding(start = 16.dp, end = 8.dp, bottom = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Line("Loved on Last.fm", body(17.sp, Classic.current, FontWeight.Bold), Modifier.weight(1f, fill = false))
                Line("AS ${loved.user.uppercase()}", label(11.sp, Classic.label), Modifier.weight(1f))
            }
        }
        Row(
            Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, bottom = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Line(songs(songs.size).lowercase(), body(13.sp, Classic.label), Modifier.weight(1f))
            for ((by, name) in listOf(LovedOrder.ARTIST to "ARTIST", LovedOrder.TITLE to "TITLE", LovedOrder.DATE to "DATE")) {
                TextButton(name, { core.lovedOrder = by }, on = order == by)
            }
        }
        val scroll = remember { core.browseScroll.getOrPut("lastfm") { LazyListState() } }
        ScrollingList(
            scroll,
            Modifier.fillMaxSize().padding(horizontal = 8.dp).lcd().padding(2.dp),
        ) {
            if (songs.isEmpty()) {
                item {
                    BasicText(
                        "No loved songs yet: love some with the heart by the song playing.",
                        Modifier.padding(12.dp),
                        body(color = Classic.dim, weight = FontWeight.Normal),
                    )
                }
            }
            itemsIndexed(songs) { index, song ->
                val chosen = index in selected
                SongRow(
                    core, song, index + 1, chosen, selecting = selected.isNotEmpty(),
                    tap = {
                        when {
                            selected.isEmpty() -> core.apply(Action.PlayLoved(song))
                            chosen -> picked = picked - index
                            else -> {
                                anchor = index
                                picked = picked + index
                            }
                        }
                    },
                    hold = {
                        val from = anchor
                        picked = if (from != null && from in selected) {
                            picked + (minOf(from, index)..maxOf(from, index))
                        } else {
                            anchor = index
                            picked + index
                        }
                    },
                )
            }
        }
    }
}

/** In place of the tab's title while songs are selected. */
@Composable
private fun SelectBar(core: Core, songs: List<LovedSong>, selected: List<Int>, all: () -> Unit, done: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(start = 8.dp, end = 8.dp, bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GlyphButton(Glyph.Close, "Stop selecting", done, Modifier.width(48.dp))
        Line(
            "${selected.size} selected",
            body(15.sp, Classic.current, FontWeight.SemiBold),
            Modifier.weight(1f).padding(start = 4.dp),
        )
        TextButton("ALL", all, enabled = selected.size < songs.size)
        if (ADDING) {
            TextButton("ADD", {
                core.apply(Action.AddLoved(selected.map { songs[it] }))
                done()
            })
        }
        GlyphButton(Glyph.Heart, "Unlove on Last.fm", {
            for (i in selected) core.apply(Action.SetLoved(songs[i].artist, songs[i].title, false))
            done()
        }, Modifier.width(48.dp), lit = true)
    }
}

/** "29 Oct 2025", the day a song was loved (UTC); blank when not known. */
private val DAY = DateTimeFormatter.ofPattern("d MMM yyyy", Locale.ENGLISH).withZone(ZoneOffset.UTC)

private fun day(unix: ULong) = if (unix == 0uL) "" else DAY.format(Instant.ofEpochSecond(unix.toLong()))

/** A song: tap plays it now, + adds it (with [ADDING]); while selecting, a tap picks it. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SongRow(
    core: Core,
    song: LovedSong,
    number: Int,
    chosen: Boolean,
    selecting: Boolean,
    tap: () -> Unit,
    hold: () -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(52.dp)
            .background(if (chosen) Classic.selected else Color.Transparent)
            .combinedClickable(onLongClick = hold, onClick = tap)
            .padding(start = 8.dp, end = if (selecting) 12.dp else 6.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(
            "$number.", Modifier.width(34.dp),
            mono(color = if (chosen) Classic.current else Classic.dim).copy(textAlign = TextAlign.End),
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Line(song.title, body(color = if (chosen) Classic.current else Classic.list))
            Line(
                listOf(song.artist, day(song.date)).filter { it.isNotEmpty() }.joinToString(" · "),
                body(13.sp, if (chosen) Color(0xFFD6DCFF) else Classic.dim, FontWeight.Normal),
            )
        }
        if (ADDING && !selecting) {
            AddButton("+", label(18.sp), "Add the song") { core.apply(Action.AddLoved(listOf(song))) }
        }
    }
}
