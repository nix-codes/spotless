package dev.spotless.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
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
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
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
import dev.spotless.core.AddAll
import dev.spotless.core.AlbumSongs
import dev.spotless.core.Artist
import dev.spotless.core.ArtistPage
import dev.spotless.core.BrowsePage
import dev.spotless.core.BrowseTab
import dev.spotless.core.ReleaseRow
import dev.spotless.core.ReleaseTab
import dev.spotless.core.SearchPage
import dev.spotless.core.SearchState
import dev.spotless.core.SimilarArtists
import dev.spotless.core.Track

/**
 * Browse: the catalog (search, results and artist pages) and YOUR
 * SPOTIFY, the account's own playlists; then the playlist ADD adds to
 * (the desktop's slim column), and a small player.
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
            GlyphButton(Glyph.Back, "Back", { core.browseBack() }, Modifier.width(48.dp))
            val artist = (browse.page as? BrowsePage.Artist)
                ?.takeIf { browse.shown == BrowseTab.SEARCH }
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
        BrowseTabs(core)
        if (browse.shown == BrowseTab.SPOTIFY) {
            YourSpotify(core, Modifier.weight(1f))
        } else {
            Catalog(core, Modifier.weight(1f))
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

/** SEARCH: the search field, then its results or an artist's page. */
@Composable
private fun Catalog(core: Core, modifier: Modifier) {
    val page = core.browse.page
    val key = when (page) {
        is BrowsePage.Home -> "home"
        is BrowsePage.Search -> "search:${page.query}"
        is BrowsePage.Artist -> "artist:${page.uri}"
    }
    val scroll = remember(key) { core.browseScroll.getOrPut(key) { LazyListState() } }
    Column(modifier) {
        if (page !is BrowsePage.Artist) {
            SearchField(core, (page as? BrowsePage.Search)?.query ?: "", findable = page is BrowsePage.Search)
        }
        if (page is BrowsePage.Artist) ArtistBar(core, page)
        if (core.browse.find != null) FindField(core, artist = page is BrowsePage.Artist)
        ScrollingList(
            scroll,
            Modifier.weight(1f).fillMaxWidth().padding(horizontal = 8.dp).lcd().padding(2.dp),
        ) {
            when (page) {
                is BrowsePage.Home -> home(core, page)
                is BrowsePage.Search -> search(core, page.query, page.page)
                is BrowsePage.Artist -> artist(core, page.page)
            }
        }
    }
}

/** SEARCH and YOUR SPOTIFY. */
@Composable
private fun BrowseTabs(core: Core) {
    val shown = core.browse.shown
    Row(
        Modifier.padding(start = 8.dp, end = 8.dp, bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        TextButton("SEARCH", { core.apply(Action.ShowBrowseTab(BrowseTab.SEARCH)) }, Modifier.weight(1f), on = shown == BrowseTab.SEARCH)
        TextButton("YOUR SPOTIFY", {
            core.apply(Action.ShowBrowseTab(BrowseTab.SPOTIFY))
            // What changed on Spotify meanwhile shows.
            core.apply(Action.ReadSpotifyPlaylists)
        }, Modifier.weight(1f), on = shown == BrowseTab.SPOTIFY)
    }
}

/** FIND opens the page's find, or closes it. */
private fun toggleFind(core: Core) = core.apply(Action.Find(if (core.browse.find != null) null else ""))

@Composable
private fun SearchField(core: Core, query: String, findable: Boolean) {
    var text by remember(query) { mutableStateOf(query) }
    val focus = LocalFocusManager.current
    Row(
        Modifier.padding(start = 8.dp, end = 8.dp, bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicTextField(
            text,
            { text = it },
            Modifier
                .weight(1f)
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
        if (findable) TextButton("FIND", { toggleFind(core) }, Modifier.height(48.dp), on = core.browse.find != null)
    }
}

/**
 * The page's find, open under the search field or the artist's tabs: what's
 * typed narrows the results, or the releases and their songs, at once.
 * CLOSE or Back closes it.
 */
@Composable
private fun FindField(core: Core, artist: Boolean) {
    var text by remember { mutableStateOf(core.browse.find.orEmpty()) }
    val focus = remember { FocusRequester() }
    val manager = LocalFocusManager.current
    LaunchedEffect(Unit) { focus.requestFocus() }
    BackHandler { core.apply(Action.Find(null)) }
    Row(
        Modifier.padding(start = 8.dp, end = 8.dp, bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicTextField(
            text,
            {
                text = it
                core.apply(Action.Find(it))
            },
            Modifier
                .weight(1f)
                .height(48.dp)
                .lcd()
                .padding(horizontal = 12.dp, vertical = 13.dp)
                .focusRequester(focus)
                .semantics { contentDescription = "Find on this page" },
            textStyle = body(16.sp, Classic.lcd),
            singleLine = true,
            cursorBrush = SolidColor(Classic.lcd),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { manager.clearFocus() }),
            decorationBox = { field ->
                val hint = if (artist) "Find a song or a release here" else "Find in these results"
                if (text.isEmpty()) BasicText(hint, style = body(16.sp, Classic.dim))
                field()
            },
        )
        TextButton("CLOSE", { core.apply(Action.Find(null)) }, Modifier.height(48.dp))
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

private fun LazyListScope.search(core: Core, query: String, page: SearchPage) {
    when (page) {
        SearchPage.Loading -> note("Searching…")
        is SearchPage.Failed -> note(page.message)
        is SearchPage.Ready -> {
            val find = core.browse.find?.trim().orEmpty()
            if (page.artists.isEmpty() && page.albums.isEmpty() && page.songs.isEmpty()) {
                if (find.isEmpty()) return note("Nothing found.")
                // Not among the results read: Spotify is asked with it added.
                note("Nothing found here has “$find”.")
                val wider = "$query $find"
                item {
                    TextButton(
                        "SEARCH SPOTIFY FOR “$wider”",
                        { core.apply(Action.Search(wider)) },
                        Modifier.padding(12.dp).height(36.dp),
                    )
                }
                return
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
                if (page.readingSongs) {
                    note("Reading more songs…")
                } else if (page.moreSongs) {
                    item {
                        TextButton(
                            "MORE SONGS",
                            { core.apply(Action.MoreSongs(query)) },
                            Modifier.padding(12.dp).height(36.dp),
                        )
                    }
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
            val find = core.browse.find?.trim().orEmpty()
            listBar(core, page)
            if (core.browse.popular) {
                if (page.popular.isEmpty()) note("No popular song has “$find”.")
                itemsIndexed(page.popular) { i, track ->
                    TrackRow(core, track, page.popularRanks[i].toInt(), track.album.name)
                }
                return
            }
            if (page.unread > 0u) note("Reading the songs, ${page.unread} releases to go…")
            if (page.releases.isEmpty()) {
                when {
                    find.isEmpty() -> note("Nothing on this tab.")
                    page.unread == 0u -> note("No song or release here has “$find”.")
                }
            }
            releases(core, page.releases, withArtist = false)
        }
    }
}

/**
 * First in an artist's list, what acts on it: the releases' order and ALL
 * SONGS on the left; on the right, over the rows' own ADD and +, ADD ALL,
 * which adds what the list shows (the tab's releases in their order, the
 * popular songs, or what the find found) to the playlist ADD adds to. It
 * isn't there while a release the find found is still being read.
 */
private fun LazyListScope.listBar(core: Core, page: ArtistPage.Ready) {
    val releases = page.releases.takeUnless { core.browse.popular }.orEmpty()
    val what = when (val addAll = page.addAll) {
        is AddAll.Releases -> if (addAll.count == 1u) "the release shown" else "the ${addAll.count} releases shown"
        is AddAll.Songs -> if (addAll.count == 1u) "the song shown" else "the ${addAll.count} songs shown"
        AddAll.Nothing, AddAll.Reading -> null
    }
    if (releases.isEmpty() && what == null) return
    item(key = "list-bar") {
        Row(
            Modifier.fillMaxWidth().padding(top = 4.dp, start = 6.dp, end = 6.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (releases.isNotEmpty()) {
                TextButton(
                    if (core.browse.newestFirst) "NEWEST FIRST" else "OLDEST FIRST",
                    { core.apply(Action.ToggleSort) },
                )
                // Every release shown opened, or all closed again.
                val open = releases.all { it.expanded }
                TextButton(
                    if (open) "HIDE SONGS" else "ALL SONGS",
                    { core.apply(Action.ExpandAlbums(if (open) emptyList() else releases.map { it.release.uri })) },
                )
            }
            Box(Modifier.weight(1f))
            if (what != null) AddButton("ADD ALL", label(), "Add $what") { core.apply(Action.AddAll) }
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
    val popular = core.browse.popular
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
                    on = !similar && !popular && core.browse.tab == tab,
                )
            }
        }
        // What shows in place of the releases, then the tools; the order
        // and ALL SONGS are over the list, with ADD ALL.
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            // Spotify's pick of the artist's most played.
            if (ready?.popular?.isNotEmpty() == true || popular) {
                TextButton("POPULAR", { core.apply(Action.ShowPopular) }, on = popular)
            }
            if (ready?.similarOffered == true) {
                TextButton("SIMILAR", { core.apply(Action.ShowSimilar(ready.artist)) }, on = similar)
            }
            Box(Modifier.weight(1f))
            if (!similar) TextButton("FIND", { toggleFind(core) }, on = core.browse.find != null)
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
                    .combinedClickable(onLongClick = { core.share(listOf(release.uri)) }) {
                        core.apply(Action.ToggleAlbum(release.uri))
                    }
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
                    TrackRow(core, track, songs.numbers[i].toInt(), null)
                }
            }
        }
    }
}

/** A release's cover or an artist's picture, a dark square until it's read. */
@Composable
internal fun Cover(core: Core, url: String?) {
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
            .combinedClickable(onLongClick = { core.share(listOf(artist.uri)) }) { core.apply(Action.OpenArtist(artist)) }
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

/** A song: tap plays it now, after the target's current song; hold shares it; + adds it. */
@Composable
private fun TrackRow(core: Core, track: Track, number: Int?, sub: String?) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(if (sub != null) 52.dp else 46.dp)
            .combinedClickable(onLongClick = { core.share(listOf(track.uri)) }) { core.apply(Action.PlayNow(track)) }
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
internal fun AddButton(
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
