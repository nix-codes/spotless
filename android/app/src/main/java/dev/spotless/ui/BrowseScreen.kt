package dev.spotless.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import kotlinx.coroutines.delay
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.foundation.lazy.LazyListState
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
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Popup
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.AlbumSongs
import dev.spotless.core.Artist
import dev.spotless.core.ArtistPage
import dev.spotless.core.BrowsePage
import dev.spotless.core.ReleaseRow
import dev.spotless.core.ReleaseTab
import dev.spotless.core.SearchPage
import dev.spotless.core.SearchState
import dev.spotless.core.SimilarArtists
import dev.spotless.core.Track

/**
 * The catalog: search, results and artist pages, the playlist ADD adds
 * to (the desktop's slim column), and a small player.
 */
@Composable
fun BrowseScreen(core: Core) {
    val browse = core.browse
    Column(Modifier.fillMaxSize().background(Classic.frame).safeDrawingPadding()) {
        Row(
            Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GlyphButton(Glyph.Back, "Back", {
                core.apply(if (browse.canGoBack) Action.Back else Action.CloseBrowse)
            }, Modifier.width(48.dp))
            val artist = (browse.page as? BrowsePage.Artist)
                ?.let { (it.page as? ArtistPage.Ready)?.artist }
            if (artist != null) {
                if (browse.covers) Cover(core, artist.image)
                Line(artist.name, body(18.sp, Classic.current, FontWeight.Bold), Modifier.weight(1f).padding(start = 4.dp))
            } else {
                BasicText("B R O W S E", Modifier.weight(1f).padding(start = 4.dp), label(spacing = 0.3.em))
            }
            // Straight to the playlists; Browse opens on this page again.
            GlyphButton(Glyph.Close, "Back to the playlists", { core.apply(Action.CloseBrowse) }, Modifier.width(48.dp))
        }
        val page = browse.page
        val key = when (page) {
            is BrowsePage.Home -> "home"
            is BrowsePage.Search -> "search:${page.query}"
            is BrowsePage.Artist -> "artist:${page.uri}"
        }
        val scroll = remember(key) { core.browseScroll.getOrPut(key) { LazyListState() } }
        if (page !is BrowsePage.Artist) SearchField(core, (page as? BrowsePage.Search)?.query ?: "")
        if (page is BrowsePage.Artist) ArtistBar(core, page)
        ScrollingList(
            scroll,
            Modifier.weight(1f).fillMaxWidth().padding(horizontal = 8.dp).lcd().padding(2.dp),
        ) {
            when (page) {
                is BrowsePage.Home -> home(core, page)
                is BrowsePage.Search -> search(core, page.page)
                is BrowsePage.Artist -> artist(core, page.page)
            }
        }
        ErrorNote(core, Modifier.padding(top = 4.dp))
        Column(
            Modifier.padding(start = 8.dp, end = 8.dp, top = 8.dp, bottom = 14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Target(core)
            MiniPlayer(core)
        }
    }
}

@Composable
private fun SearchField(core: Core, query: String) {
    var text by remember(query) { mutableStateOf(query) }
    val focus = LocalFocusManager.current
    Box(Modifier.padding(start = 8.dp, end = 8.dp, bottom = 8.dp)) {
        BasicTextField(
            text,
            { text = it },
            Modifier
                .fillMaxWidth()
                .height(48.dp)
                .lcd()
                .padding(horizontal = 12.dp, vertical = 13.dp)
                .semantics { contentDescription = "Search, or a Spotify link" },
            textStyle = body(16.sp, Classic.lcd),
            singleLine = true,
            cursorBrush = SolidColor(Classic.lcd),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = {
                focus.clearFocus()
                core.apply(Action.Search(text))
            }),
            decorationBox = { field ->
                if (text.isEmpty()) BasicText("Artist, album, song or link", style = body(16.sp, Classic.dim))
                field()
            },
        )
    }
}

private fun LazyListScope.note(text: String) = item {
    BasicText(text, Modifier.padding(12.dp), body(color = Classic.dim, weight = FontWeight.Normal))
}

private fun LazyListScope.section(title: String) = item {
    BasicText(title, Modifier.padding(start = 12.dp, top = 14.dp, bottom = 6.dp), label(12.sp, Classic.label, 0.14.em))
}

private fun LazyListScope.home(core: Core, page: BrowsePage.Home) {
    when (core.browse.search) {
        SearchState.NO_APP -> note(
            "Search needs your own Spotify app, which Settings sets up. " +
                "A pasted Spotify link adds its songs meanwhile.",
        )
        SearchState.SIGNING_IN -> note("Approve search in the browser page that opened.")
        else -> {}
    }
    if (core.browse.search != SearchState.NO_APP && page.searches.isNotEmpty()) {
        item {
            Row(
                Modifier.fillMaxWidth().padding(start = 12.dp, end = 6.dp, top = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                BasicText("RECENT SEARCHES", Modifier.weight(1f), label(12.sp, Classic.label, 0.14.em))
                TextButton("CLEAR", { core.apply(Action.ForgetSearch(null)) }, Modifier.height(36.dp))
            }
        }
        items(page.searches, key = { "search:$it" }) { SearchRow(core, it) }
    }
    if (page.recent.isNotEmpty()) {
        section("RECENT ARTISTS")
        items(page.recent, key = { it.uri }) { ArtistRow(core, it) }
    }
}

private fun LazyListScope.search(core: Core, page: SearchPage) {
    when (page) {
        SearchPage.Loading -> note("Searching…")
        is SearchPage.Failed -> note(page.message)
        is SearchPage.Ready -> {
            if (page.artists.isEmpty() && page.albums.isEmpty() && page.songs.isEmpty()) {
                note("Nothing found.")
            }
            if (page.artists.isNotEmpty()) {
                section("ARTISTS")
                itemsIndexed(page.artists) { _, artist -> ArtistRow(core, artist) }
            }
            if (page.albums.isNotEmpty()) {
                section("ALBUMS")
                releases(core, page.albums, withArtist = true)
            }
            if (page.songs.isNotEmpty()) {
                section("SONGS")
                itemsIndexed(page.songs) { _, track ->
                    TrackRow(core, track, null, "${artists(track)} · ${track.album.name}")
                }
            }
        }
    }
}

private fun LazyListScope.artist(core: Core, page: ArtistPage) {
    when (page) {
        ArtistPage.Loading -> note("Reading the discography…")
        is ArtistPage.Failed -> note(page.message)
        is ArtistPage.Ready -> {
            if (core.browse.similar && page.similarOffered) {
                similar(core, page.similar)
                return
            }
            if (page.releases.isEmpty()) note("Nothing on this tab.")
            releases(core, page.releases, withArtist = false)
        }
    }
}

/** Artists Last.fm's listeners play alongside this one; tap opens one. */
private fun LazyListScope.similar(core: Core, similar: SimilarArtists) {
    when (similar) {
        SimilarArtists.NotRead, SimilarArtists.Loading -> note("Asking Last.fm…")
        is SimilarArtists.Failed -> note(similar.message)
        is SimilarArtists.Ready -> {
            if (similar.names.isEmpty()) note("Last.fm knows no artists like this one.")
            items(similar.names, key = { "similar:$it" }) { name ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .height(52.dp)
                        .clickable { core.apply(Action.OpenSimilar(name)) }
                        .padding(horizontal = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Line(name, body(), Modifier.weight(1f))
                    if (core.browse.finding == name) {
                        BasicText("FINDING…", style = label(12.sp, Classic.dim))
                    } else {
                        Box(Modifier.size(18.dp).drawBehind { glyph(Glyph.Closed, Classic.dim) })
                    }
                }
            }
            if (similar.names.isNotEmpty()) {
                note("From Last.fm: what its listeners play alongside this artist.")
            }
        }
    }
}

/** The artist page's tabs and order. */
@Composable
private fun ArtistBar(core: Core, page: BrowsePage.Artist) {
    val ready = page.page as? ArtistPage.Ready
    val counts = ready?.counts
    val similar = core.browse.similar && ready?.similarOffered == true
    val tabs = listOf(
        ReleaseTab.ALL to ("ALL" to counts?.all),
        ReleaseTab.ALBUMS to ("ALBUMS" to counts?.albums),
        ReleaseTab.SINGLES_AND_EPS to ("SINGLES" to counts?.singlesAndEps),
        ReleaseTab.APPEARS_ON to ("APPEARS" to counts?.appearsOn),
    )
    Column(Modifier.padding(start = 8.dp, end = 8.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            for ((tab, name) in tabs) {
                val (text, count) = name
                TextButton(
                    if (count != null) "$text $count" else text,
                    { core.apply(Action.SetTab(tab)) },
                    Modifier.weight(1f),
                    on = !similar && core.browse.tab == tab,
                )
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            if (!similar) {
                TextButton(
                    if (core.browse.newestFirst) "NEWEST FIRST" else "OLDEST FIRST",
                    { core.apply(Action.ToggleSort) },
                )
            }
            // Every release of the tab opened, or all closed again.
            val releases = ready?.releases.orEmpty()
            if (!similar && releases.isNotEmpty()) {
                val open = releases.all { it.expanded }
                TextButton(
                    if (open) "HIDE SONGS" else "ALL SONGS",
                    { core.apply(Action.ExpandAlbums(if (open) emptyList() else releases.map { it.release.uri })) },
                )
            }
            Box(Modifier.weight(1f))
            if (ready?.similarOffered == true) {
                TextButton("SIMILAR", { core.apply(Action.ShowSimilar(ready.artist)) }, on = similar)
            }
            TextButton("PICS", { core.apply(Action.ToggleCovers) }, on = core.browse.covers)
        }
    }
}

private fun LazyListScope.releases(core: Core, rows: List<ReleaseRow>, withArtist: Boolean) {
    // Not keyed by URI: Spotify can list a release twice.
    for (row in rows) {
        val release = row.release
        item {
            val sub = buildString {
                if (withArtist) append(release.artists).append(" · ")
                append(release.year).append(" · ").append(songs(release.songs.toInt()).lowercase())
            }
            Row(
                Modifier
                    .fillMaxWidth()
                    .height(56.dp)
                    .background(if (row.expanded) Classic.selected else Color.Transparent)
                    .clickable { core.apply(Action.ToggleAlbum(release.uri)) }
                    .padding(start = 12.dp, end = 6.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val glyph = if (row.expanded) Glyph.Open else Glyph.Closed
                val tint = if (row.expanded) Classic.current else Classic.dim
                Box(Modifier.size(14.dp).drawBehind { glyph(glyph, tint) })
                if (core.browse.covers) Cover(core, release.coverUrl)
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Line(release.title, body(color = if (row.expanded) Classic.current else Classic.list))
                    Line(sub, body(13.sp, if (row.expanded) Color(0xFFD6DCFF) else Classic.dim, FontWeight.Normal))
                }
                AddButton("ADD", label(), "Add the album", Modifier.width(64.dp)) { core.apply(Action.AddAlbum(release.uri)) }
            }
        }
        if (row.expanded) {
            when (val songs = row.songs) {
                AlbumSongs.NotRead, AlbumSongs.Loading -> note("Reading the songs…")
                is AlbumSongs.Failed -> note(songs.message)
                is AlbumSongs.Ready -> itemsIndexed(songs.tracks) { i, track ->
                    TrackRow(core, track, i + 1, null)
                }
            }
        }
    }
}

/** A release's cover or an artist's picture, a dark square until it's read. */
@Composable
private fun Cover(core: Core, url: String?) {
    val image = core.cover(url)
    if (image != null) {
        Image(image, null, Modifier.size(40.dp))
    } else {
        Box(Modifier.size(40.dp).background(Classic.playingRow))
    }
}

@Composable
private fun ArtistRow(core: Core, artist: Artist) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(52.dp)
            .clickable { core.apply(Action.OpenArtist(artist)) }
            .padding(horizontal = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (core.browse.covers) Cover(core, artist.image)
        Line(artist.name, body(), Modifier.weight(1f))
        Box(Modifier.size(18.dp).drawBehind { glyph(Glyph.Closed, Classic.dim) })
    }
}

/** A recent search: tap runs it again, ✕ forgets it. */
@Composable
private fun SearchRow(core: Core, query: String) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(48.dp)
            .clickable { core.apply(Action.Search(query)) }
            .padding(start = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(16.dp).drawBehind { glyph(Glyph.Search, Classic.dim) })
        Line(query, body(), Modifier.weight(1f))
        Box(
            Modifier
                .size(48.dp)
                .clickable { core.apply(Action.ForgetSearch(query)) }
                .semantics { contentDescription = "Forget $query" },
            contentAlignment = Alignment.Center,
        ) {
            Box(Modifier.size(14.dp).drawBehind { glyph(Glyph.Close, Classic.dim) })
        }
    }
}

private fun artists(track: Track) = track.artists.joinToString(", ") { it.name }

/** A song: tap plays it now, after the target's current song; + adds it. */
@Composable
private fun TrackRow(core: Core, track: Track, number: Int?, sub: String?) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(if (sub != null) 52.dp else 46.dp)
            .clickable { core.apply(Action.PlayNow(track)) }
            .padding(start = if (number != null) 16.dp else 12.dp, end = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (number != null) {
            BasicText("$number.", Modifier.width(28.dp), mono(color = Classic.dim).copy(textAlign = TextAlign.End))
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Line(track.title, body())
            if (sub != null) Line(sub, body(13.sp, Classic.dim, FontWeight.Normal))
        }
        BasicText(clock(track.durationMs.toLong()), style = mono())
        AddButton("+", label(18.sp), "Add the song") { core.apply(Action.AddTracks(listOf(track))) }
    }
}

/**
 * ADD or +: stays in, lit with a tick, for a moment after each press, so a
 * press shows even though it can be pressed again.
 */
@Composable
private fun AddButton(
    text: String,
    style: TextStyle,
    description: String,
    modifier: Modifier = Modifier,
    add: () -> Unit,
) {
    var presses by remember { mutableIntStateOf(0) }
    var done by remember { mutableStateOf(false) }
    LaunchedEffect(presses) {
        if (presses == 0) return@LaunchedEffect
        done = true
        delay(1200)
        done = false
    }
    ChromeButton({ add(); presses++ }, modifier, on = done, description = description) {
        if (done) {
            Box(Modifier.size(22.dp).drawBehind { glyph(Glyph.Check, Classic.lcd) })
        } else {
            BasicText(text, style = style)
        }
    }
}

/** The playlist ADD adds to; tap to choose another. */
@Composable
private fun Target(core: Core) {
    var choosing by remember { mutableStateOf(false) }
    val target = core.tabs.tabs.firstOrNull { it.id == core.browse.target }
    // Songs just added to the target, shown for a moment instead of its count.
    val count = target?.songs?.toInt()
    var seen by remember(target?.id) { mutableStateOf(count) }
    var added by remember(target?.id) { mutableIntStateOf(0) }
    LaunchedEffect(target?.id, count) {
        val before = seen
        seen = count
        if (count == null || before == null || count <= before) return@LaunchedEffect
        added += count - before
        delay(2000)
        added = 0
    }
    Box {
        ChromeButton({ choosing = !choosing }, Modifier.fillMaxWidth().height(48.dp)) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                BasicText("ADD TO", style = label(12.sp, Classic.label, 0.14.em))
                Line(target?.name ?: "", body(color = Classic.lcd), Modifier.weight(1f))
                if (added > 0) {
                    BasicText("+$added", style = mono(14.sp, Classic.lcd))
                } else {
                    target?.let { BasicText(songs(it.songs.toInt()).lowercase(), style = body(13.sp, Classic.label)) }
                }
                Box(Modifier.size(16.dp).drawBehind { glyph(if (choosing) Glyph.Open else Glyph.Closed, Classic.chrome) })
            }
        }
        if (choosing) {
            Popup(alignment = Alignment.BottomStart, onDismissRequest = { choosing = false }) {
                Column(Modifier.width(260.dp).background(Classic.frame).bevel().padding(4.dp)) {
                    for (tab in core.tabs.tabs) {
                        TextButton(
                            tab.name,
                            {
                                core.apply(Action.SetTarget(tab.id))
                                choosing = false
                            },
                            Modifier.fillMaxWidth(),
                            on = tab.id == core.browse.target,
                        )
                    }
                }
            }
        }
    }
}
