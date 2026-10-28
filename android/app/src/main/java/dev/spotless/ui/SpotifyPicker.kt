package dev.spotless.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
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
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.SpotifyPlaylists
import dev.spotless.core.SpotifyTarget
import dev.spotless.core.Track

/**
 * Where songs copied to Spotify go: one of the account's own playlists,
 * found by typing part of its name, or a new one, named by what's typed
 * ([name], the songs' tab's, when nothing is). With [saving], only a new
 * one: a whole tab saved to Spotify. A copy is made once; the tab isn't
 * kept in step with it after.
 */
@Composable
fun SpotifyPicker(core: Core, tracks: List<Track>, name: String, saving: Boolean, close: () -> Unit, done: () -> Unit) {
    var text by remember { mutableStateOf(if (saving) name else "") }
    val focus = LocalFocusManager.current
    val copy = { target: SpotifyTarget ->
        focus.clearFocus()
        core.apply(Action.CopyToSpotify(target, tracks))
        done()
    }
    val new = text.trim().ifEmpty { name }
    // The playlists as they are now: one made on another device shows.
    LaunchedEffect(saving) { if (!saving) core.apply(Action.ReadSpotifyPlaylists) }
    Column(
        Modifier.fillMaxWidth().padding(8.dp).background(Classic.frame).bevel().padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            BasicText(
                if (saving) "SAVE TO A NEW SPOTIFY PLAYLIST" else "COPY ${songs(tracks.size)} TO SPOTIFY",
                Modifier.weight(1f),
                label(12.sp, Classic.label, 0.06.em),
            )
            GlyphButton(Glyph.Close, "Cancel", {
                focus.clearFocus()
                close()
            }, Modifier.width(48.dp))
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            BasicTextField(
                text,
                { text = it },
                Modifier
                    .weight(1f)
                    .height(44.dp)
                    .lcd()
                    .padding(horizontal = 12.dp, vertical = 12.dp)
                    .semantics { contentDescription = if (saving) "New playlist's name" else "Find a playlist, or name a new one" },
                textStyle = body(16.sp, Classic.lcd),
                singleLine = true,
                cursorBrush = SolidColor(Classic.lcd),
                keyboardOptions = KeyboardOptions(
                    capitalization = KeyboardCapitalization.Sentences,
                    imeAction = if (saving) ImeAction.Done else ImeAction.Search,
                ),
                keyboardActions = KeyboardActions(
                    onDone = { if (new.isNotBlank()) copy(SpotifyTarget.New(new)) },
                    onSearch = { focus.clearFocus() },
                ),
                decorationBox = { field ->
                    if (text.isEmpty()) BasicText(if (saving) "Name" else "Find, or name a new one", style = body(16.sp, Classic.dim))
                    field()
                },
            )
            if (saving) TextButton("SAVE", { copy(SpotifyTarget.New(new)) }, Modifier.height(44.dp), enabled = new.isNotBlank())
        }
        if (!saving) Choices(core, text.trim(), new, copy)
    }
}

/** A new playlist named [new], then the account's own whose names have [find] in them. */
@Composable
private fun Choices(core: Core, find: String, new: String, copy: (SpotifyTarget) -> Unit) {
    val playlists = core.browse.yourSpotify.playlists
    // Liked Songs is liked song by song, and others' playlists can't change.
    val own = (playlists as? SpotifyPlaylists.Ready)?.playlists.orEmpty()
        .filter { it.owner == null && !it.liked && it.name.contains(find, ignoreCase = true) }
    LazyColumn(Modifier.fillMaxWidth().heightIn(max = 240.dp).lcd().padding(2.dp)) {
        item(key = "new") {
            Choice("+ NEW  “$new”", "new playlist", enabled = new.isNotBlank()) { copy(SpotifyTarget.New(new)) }
        }
        when (playlists) {
            SpotifyPlaylists.Loading -> item { Note("Reading your playlists…") }
            is SpotifyPlaylists.Failed -> item { Note(playlists.message) }
            is SpotifyPlaylists.Ready -> items(own, key = { it.uri }) { playlist ->
                Choice(playlist.name, songs(playlist.songs.toInt()).lowercase()) {
                    copy(SpotifyTarget.Playlist(playlist.uri, playlist.name))
                }
            }
        }
    }
}

@Composable
private fun Choice(name: String, about: String, enabled: Boolean = true, tap: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(48.dp)
            .clickable(enabled = enabled, onClick = tap)
            .padding(horizontal = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Line(name, body(color = if (enabled) Classic.list else Classic.dim), Modifier.weight(1f))
        BasicText(about, style = body(13.sp, Classic.dim, FontWeight.Normal))
    }
}

@Composable
private fun Note(text: String) {
    BasicText(text, Modifier.padding(12.dp), body(13.sp, Classic.dim, FontWeight.Normal))
}
