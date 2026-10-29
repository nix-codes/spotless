package dev.spotless.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.AlbumSongs
import dev.spotless.core.SpotifyPlaylist
import dev.spotless.core.SpotifyPlaylists
import dev.spotless.core.Track

/**
 * Browse's YOUR SPOTIFY: the account's Spotify playlists, Liked Songs and
 * its own first, then those it follows, with a field to find one by name.
 * Tapping one shows its songs, to add whole or song by song (a tap plays
 * a song now, as in SEARCH). Holding a song selects it, holding another
 * then selects the ones between; selected songs are added, or, on the
 * account's own playlists and Liked Songs, taken out on Spotify. A
 * playlist is deleted, or unfollowed if it's someone else's, from its
 * songs' page. UNDO puts back either.
 */
@Composable
fun YourSpotify(core: Core, modifier: Modifier) {
    val yours = core.browse.yourSpotify
    Column(modifier) {
        UndoBar(core)
        when (val playlists = yours.playlists) {
            SpotifyPlaylists.Loading -> Note("Reading your playlists…")
            is SpotifyPlaylists.Failed -> Note(playlists.message)
            is SpotifyPlaylists.Ready -> {
                val open = playlists.playlists.firstOrNull { it.uri == yours.open }
                if (core.spotifySongs && open != null) {
                    PlaylistSongs(core, open, yours.songs)
                } else {
                    PlaylistList(core, playlists.playlists)
                }
            }
        }
    }
}

/** What was just taken out on Spotify, with UNDO, while that's the latest news. */
@Composable
private fun UndoBar(core: Core) {
    val undo = core.browse.yourSpotify.undo ?: return
    val message = core.message?.takeIf { !it.error && it.text == undo } ?: return
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(message.text, Modifier.weight(1f), body(13.sp, Classic.chrome, FontWeight.Normal))
        TextButton("UNDO", { core.apply(Action.UndoSpotifyRemoval) }, Modifier.height(40.dp))
    }
}

@Composable
private fun Note(text: String) {
    Box(Modifier.fillMaxWidth().padding(horizontal = 8.dp).lcd().padding(2.dp)) {
        BasicText(text, Modifier.padding(12.dp), body(color = Classic.dim, weight = FontWeight.Normal))
    }
}

private fun LazyListScope.note(text: String) = item {
    BasicText(text, Modifier.padding(12.dp), body(color = Classic.dim, weight = FontWeight.Normal))
}

/** Above YOURS or FOLLOWED: a band as over an album's songs in a tab, with how many there are. */
private fun LazyListScope.group(title: String, count: Int, first: Boolean) = item(key = "group:$title") {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = if (first) 0.dp else 16.dp)
            .height(34.dp)
            .background(Classic.playingRow)
            .padding(start = 10.dp, end = 14.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.width(3.dp).height(14.dp).background(Classic.lcd))
        BasicText(title, Modifier.weight(1f), label(13.sp, Classic.lcd, 0.14.em))
        BasicText("$count", style = mono(13.sp, Classic.dim))
    }
}

private fun mine(playlist: SpotifyPlaylist) = playlist.owner == null

/** "38 songs", after whose it is when that's someone else ("by Marta K."), or with [yours] "YOURS". */
private fun about(playlist: SpotifyPlaylist, yours: Boolean = false) = listOfNotNull(
    playlist.owner?.let { "by $it" } ?: "YOURS".takeIf { yours },
    songs(playlist.songs.toInt()).lowercase(),
).joinToString(" · ")

@Composable
private fun PlaylistList(core: Core, playlists: List<SpotifyPlaylist>) {
    var find by remember { mutableStateOf("") }
    val found = playlists.filter { it.name.contains(find.trim(), ignoreCase = true) }
    FindField(find) { find = it }
    val scroll = remember { core.browseScroll.getOrPut("your-spotify") { LazyListState() } }
    ScrollingList(
        scroll,
        Modifier.fillMaxSize().padding(horizontal = 8.dp).lcd().padding(2.dp),
    ) {
        if (found.isEmpty()) note(if (playlists.isEmpty()) "No playlists on your Spotify." else "No playlist by that name.")
        val open = { playlist: SpotifyPlaylist ->
            core.apply(Action.OpenSpotifyPlaylist(playlist.uri))
            core.spotifySongs = true
        }
        // Liked Songs on its own above the bands: it isn't a playlist of the account's.
        val (liked, rest) = found.partition { it.liked }
        items(liked, key = { it.uri }) { playlist -> PlaylistRow(playlist) { open(playlist) } }
        val groups = listOf("YOURS" to rest.filter(::mine), "FOLLOWED" to rest.filterNot(::mine))
            .filter { (_, these) -> these.isNotEmpty() }
        for ((i, group) in groups.withIndex()) {
            val (title, these) = group
            group(title, these.size, first = i == 0 && liked.isEmpty())
            items(these, key = { it.uri }) { playlist -> PlaylistRow(playlist) { open(playlist) } }
        }
    }
}

@Composable
private fun FindField(text: String, change: (String) -> Unit) {
    val focus = LocalFocusManager.current
    Box(Modifier.padding(start = 8.dp, end = 8.dp, bottom = 8.dp)) {
        BasicTextField(
            text,
            change,
            Modifier
                .fillMaxWidth()
                .height(48.dp)
                .lcd()
                .padding(horizontal = 12.dp, vertical = 13.dp)
                .semantics { contentDescription = "Find a playlist" },
            textStyle = body(16.sp, Classic.lcd),
            singleLine = true,
            cursorBrush = SolidColor(Classic.lcd),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { focus.clearFocus() }),
            decorationBox = { field ->
                if (text.isEmpty()) BasicText("Find a playlist", style = body(16.sp, Classic.dim))
                field()
            },
        )
    }
}

@Composable
private fun PlaylistRow(playlist: SpotifyPlaylist, open: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(56.dp)
            .clickable(onClick = open)
            .padding(start = 12.dp, end = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Line(playlist.name, body())
            Line(about(playlist), body(13.sp, Classic.dim, FontWeight.Normal))
        }
        if (playlist.private) {
            Box(Modifier.size(16.dp).semantics { contentDescription = "Private" }.drawBehind { glyph(Glyph.Lock, Classic.dim) })
        }
        Box(Modifier.size(18.dp).drawBehind { glyph(Glyph.Closed, Classic.dim) })
    }
}

/** A playlist's songs, under its name and what can be done with it. */
@Composable
private fun PlaylistSongs(core: Core, playlist: SpotifyPlaylist, songs: AlbumSongs) {
    // The songs' indices; the selection goes once some are taken out.
    var picked by remember(playlist.uri) { mutableStateOf(emptySet<Int>()) }
    var anchor by remember(playlist.uri) { mutableStateOf<Int?>(null) }
    val tracks = (songs as? AlbumSongs.Ready)?.tracks.orEmpty()
    val selected = picked.filter { it in tracks.indices }.sorted()
    val done = { picked = emptySet() }
    BackHandler(enabled = selected.isNotEmpty()) { done() }
    // Left open before a restart, its songs not read yet.
    LaunchedEffect(playlist.uri, songs is AlbumSongs.NotRead) {
        if (songs is AlbumSongs.NotRead) core.apply(Action.OpenSpotifyPlaylist(playlist.uri))
    }
    if (selected.isNotEmpty()) {
        SelectBar(core, playlist, tracks, selected, { picked = tracks.indices.toSet() }, done)
    } else {
        PlaylistBar(core, playlist)
    }
    val scroll = remember(playlist.uri) { core.browseScroll.getOrPut("your-spotify:${playlist.uri}") { LazyListState() } }
    ScrollingList(
        scroll,
        Modifier.fillMaxSize().padding(horizontal = 8.dp).lcd().padding(2.dp),
    ) {
        when (songs) {
            AlbumSongs.NotRead, AlbumSongs.Loading -> note("Reading the songs…")
            is AlbumSongs.Failed -> note(songs.message)
            is AlbumSongs.Ready -> {
                if (tracks.isEmpty()) note("No songs in this playlist.")
                itemsIndexed(tracks) { index, track ->
                    val chosen = index in selected
                    SongRow(
                        core, track, index + 1, chosen, selecting = selected.isNotEmpty(),
                        tap = {
                            when {
                                selected.isEmpty() -> core.apply(Action.PlayNow(track))
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
}

/** The playlist's name and whose it is, ADD for all its songs, and DELETE or UNFOLLOW. */
@Composable
private fun PlaylistBar(core: Core, playlist: SpotifyPlaylist) {
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Line(playlist.name, body(17.sp, Classic.current, FontWeight.Bold))
            Line(about(playlist, yours = true), body(13.sp, Classic.label, FontWeight.Normal))
        }
        if (!playlist.liked) {
            TextButton(if (mine(playlist)) "DELETE" else "UNFOLLOW", {
                core.apply(Action.UnlistSpotifyPlaylist(playlist.uri))
                core.spotifySongs = false
            }, Modifier.height(44.dp))
        }
        AddButton("ADD", label(), "Add all its songs", Modifier.width(64.dp).height(44.dp)) {
            core.apply(Action.AddAlbum(playlist.uri))
        }
    }
}

/** In place of the playlist's bar while songs are selected. */
@Composable
private fun SelectBar(
    core: Core,
    playlist: SpotifyPlaylist,
    tracks: List<Track>,
    selected: List<Int>,
    all: () -> Unit,
    done: () -> Unit,
) {
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
        TextButton("ALL", all, enabled = selected.size < tracks.size)
        TextButton("ADD", {
            core.apply(Action.AddTracks(selected.map { tracks[it] }))
            done()
        })
        // Only the account's own playlists and Liked Songs can change.
        GlyphButton(Glyph.Trash, "Remove from this playlist on Spotify", {
            core.apply(Action.RemoveFromSpotify(playlist.uri, selected.map { it.toUInt() }))
            done()
        }, Modifier.width(48.dp), enabled = mine(playlist))
    }
}

/** A song: tap plays it now, + adds it; while selecting, a tap picks it. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SongRow(
    core: Core,
    track: Track,
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
            Line(track.title, body(color = if (chosen) Classic.current else Classic.list))
            Line(
                "${track.artists.joinToString(", ") { it.name }} · ${track.album.name}",
                body(13.sp, if (chosen) Color(0xFFD6DCFF) else Classic.dim, FontWeight.Normal),
            )
        }
        BasicText(clock(track.durationMs.toLong()), style = mono())
        if (!selecting) AddButton("+", label(18.sp), "Add the song") { core.apply(Action.AddTracks(listOf(track))) }
    }
}
