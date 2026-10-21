package dev.spotless.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import java.util.Locale
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import dev.spotless.Core
import dev.spotless.core.Action
import dev.spotless.core.LastFm
import dev.spotless.core.SearchState
import dev.spotless.core.Session
import dev.spotless.core.Store

private const val DASHBOARD = "https://developer.spotify.com/dashboard"
private const val LASTFM_CREATE = "https://www.last.fm/api/account/create"
/** The redirect a personal app registers (core/src/spotify/webapi.rs). */
private const val REDIRECT_URI = "http://127.0.0.1:8989/login"

/** The account, search's app, Last.fm, levelling and the caches. */
@Composable
fun SettingsScreen(core: Core, close: () -> Unit) {
    LaunchedEffect(Unit) { core.apply(Action.MeasureStorage) }
    Column(Modifier.fillMaxSize().background(Classic.frame).safeDrawingPadding()) {
        Row(
            Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GlyphButton(Glyph.Back, "Back to the playlists", close, Modifier.width(48.dp))
            BasicText("S E T T I N G S", Modifier.weight(1f).padding(start = 4.dp), label(spacing = 0.3.em))
        }
        core.message?.let {
            Line(
                it.text, label(12.sp, if (it.error) Color(0xFFFFB000) else Classic.label, 0.02.em),
                Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            )
        }
        Column(
            Modifier
                .weight(1f)
                .imePadding()
                .verticalScroll(rememberScrollState())
                .padding(start = 8.dp, end = 8.dp, bottom = 16.dp),
        ) {
            Account(core, close)
            Search(core)
            LastFmSection(core)
            Playback(core)
            Storage(core)
        }
    }
}

@Composable
private fun Account(core: Core, close: () -> Unit) {
    Section("ACCOUNT", top = 4) {
        val user = (core.session as? Session.Ready)?.user ?: ""
        Item("Spotify") { Value(user) }
        Item("") {
            TextButton("SIGN OUT", {
                close()
                core.apply(Action.SignOut)
            })
        }
    }
}

@Composable
private fun Search(core: Core) {
    val settings = core.settings
    val (status, ready) = when (settings.search) {
        SearchState.NO_APP -> "NOT SET UP" to false
        SearchState.SIGNED_OUT -> "NOT APPROVED YET" to false
        SearchState.SIGNING_IN -> "WAITING FOR YOUR BROWSER" to false
        SearchState.READY -> "READY" to true
    }
    var typed by remember(settings.searchApp) { mutableStateOf(settings.searchApp) }
    val id = typed.trim()
    val valid = id.isEmpty() || isKey(id)
    val changed = id != settings.searchApp
    val save = { if (changed && valid) core.apply(Action.SetSearchApp(id)) }
    Section("SEARCH") {
        Item("Search") { Value(status, ready) }
        Note("Besides signing in, Spotify only lets Spotless search through a Spotify app of your own.")
        Field(typed, { typed = it }, "Client ID", onDone = save)
        Item("") { TextButton("SAVE", save, enabled = changed && valid) }
        if (!valid) Note("That isn't a Client ID: it is 32 letters and digits, on the app's page.")
        Steps(
            "1." to { Link("Create an app at developer.spotify.com/dashboard", DASHBOARD) },
            "2." to { Copy("Add the Redirect URI", REDIRECT_URI) },
            "3." to { Note("Tick Web API, then paste the app's Client ID above.", inset = false) },
        )
    }
}

@Composable
private fun LastFmSection(core: Core) {
    val settings = core.settings
    val lastfm = settings.lastfm
    val (status, connected) = when (lastfm) {
        LastFm.Off -> "NOT CONNECTED" to false
        LastFm.SigningIn -> "WAITING FOR YOUR BROWSER" to false
        is LastFm.Connected -> "CONNECTED AS ${lastfm.user.uppercase()}" to true
    }
    var key by remember(settings.lastfmKey) { mutableStateOf(settings.lastfmKey) }
    var secret by remember(settings.lastfmSecret) { mutableStateOf(settings.lastfmSecret) }
    val (k, s) = key.trim() to secret.trim()
    val valid = isKey(k) && isKey(s)
    val changed = k != settings.lastfmKey || s != settings.lastfmSecret
    val ready = valid && lastfm != LastFm.SigningIn && (changed || !connected)
    val connect = { if (ready) core.apply(Action.ConnectLastFm(k, s)) }
    Section("LAST.FM") {
        Item("Scrobbling") {
            Value(status, connected)
            if (connected) TextButton("DISCONNECT", { core.apply(Action.DisconnectLastFm) })
        }
        Note("Spotless scrobbles the songs you play through a Last.fm API account of your own.")
        Field(key, { key = it }, "API key", next = true)
        Field(secret, { secret = it }, "Shared secret", hidden = true, onDone = connect)
        Item("") { TextButton("CONNECT", connect, enabled = ready) }
        if ((k.isNotEmpty() || s.isNotEmpty()) && !valid) {
            Note("Both are 32 letters and digits, on your API account's page.")
        } else if (connected && changed) {
            Note("CONNECT to scrobble through this account instead.")
        }
        Steps(
            "1." to { Link("Create an API account at last.fm/api/account/create", LASTFM_CREATE) },
            "2." to { Note("Any application name will do. Leave the callback URL empty.", inset = false) },
            "3." to {
                Note(
                    "Paste its API key and shared secret above, then CONNECT and allow Spotless in your browser.",
                    inset = false,
                )
            },
        )
    }
}

@Composable
private fun Playback(core: Core) {
    val on = core.settings.levelling
    Section("PLAYBACK") {
        Item("Volume levelling") { Switch(on) { core.apply(Action.SetLevelling(it)) } }
        Note("Evens out loudness between songs and albums.")
    }
}

@Composable
private fun Storage(core: Core) {
    val settings = core.settings
    val sizes = settings.audioCacheSizes
    val at = sizes.indexOf(settings.audioCacheMb).let { if (it < 0) 2 else it }
    Section("STORAGE") {
        Item("Audio cache") { Switch(settings.audioCache) { core.apply(Action.SetAudioCache(it)) } }
        Item("Cache size") {
            TextButton("−", { core.apply(Action.SetAudioCacheSize(sizes[at - 1])) }, enabled = at > 0)
            Value(size(settings.audioCacheMb.toLong() * 1024 * 1024), Modifier.width(64.dp))
            TextButton("+", { core.apply(Action.SetAudioCacheSize(sizes[at + 1])) }, enabled = at < sizes.lastIndex)
        }
        Note("The switch and the size take effect the next time Spotless starts.")
        Item("Songs cached") {
            Value(settings.audioBytes?.let { size(it.toLong()) } ?: "…")
            TextButton("CLEAR", { core.apply(Action.ClearCache(Store.AUDIO)) })
        }
        Item("Covers") {
            Value(settings.coverBytes?.let { size(it.toLong()) } ?: "…")
            TextButton("CLEAR", { core.apply(Action.ClearCache(Store.COVERS)) })
        }
    }
}

@Composable
private fun Section(title: String, top: Int = 18, content: @Composable ColumnScope.() -> Unit) {
    BasicText(
        title,
        Modifier.padding(start = 4.dp, top = top.dp, bottom = 8.dp),
        label(12.sp, Classic.label, 0.14.em),
    )
    Column(Modifier.fillMaxWidth().lcd().padding(vertical = 4.dp), content = content)
}

/** A setting: its name, then its value and buttons at the end. */
@Composable
private fun Item(name: String, content: @Composable RowScope.() -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(start = 12.dp, end = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(name, Modifier.weight(1f), body(15.sp, Classic.chrome, FontWeight.Normal))
        content()
    }
}

@Composable
private fun Value(text: String, lit: Boolean = true) {
    BasicText(text, style = mono(14.sp, if (lit) Classic.lcd else Classic.dim))
}

@Composable
private fun Value(text: String, modifier: Modifier) {
    BasicText(text, modifier, mono(14.sp, Classic.lcd).copy(textAlign = TextAlign.Center))
}

@Composable
private fun Note(text: String, inset: Boolean = true) {
    BasicText(
        text,
        if (inset) Modifier.padding(start = 12.dp, end = 12.dp, bottom = 10.dp) else Modifier,
        body(13.sp, Classic.dim, FontWeight.Normal).copy(lineHeight = 18.sp),
    )
}

/** ON and OFF, the one in force sunk and lit. */
@Composable
private fun Switch(on: Boolean, set: (Boolean) -> Unit) {
    Row {
        TextButton("ON", { if (!on) set(true) }, Modifier.width(52.dp).height(40.dp), on = on)
        TextButton("OFF", { if (on) set(false) }, Modifier.width(52.dp).height(40.dp), on = !on, lit = false)
    }
}

/** A sunk field for a key: 32 letters and digits. */
@Composable
private fun Field(
    text: String,
    change: (String) -> Unit,
    name: String,
    hidden: Boolean = false,
    next: Boolean = false,
    onDone: () -> Unit = {},
) {
    val focus = LocalFocusManager.current
    BasicText(name, Modifier.padding(start = 12.dp, top = 4.dp, bottom = 6.dp), body(13.sp, Classic.label))
    BasicTextField(
        text,
        change,
        Modifier
            .fillMaxWidth()
            .padding(start = 12.dp, end = 12.dp, bottom = 8.dp)
            .height(48.dp)
            .background(Color.Black)
            .bevel(raised = false)
            .padding(horizontal = 12.dp, vertical = 14.dp)
            .semantics { contentDescription = name },
        textStyle = mono(15.sp, Classic.lcd),
        singleLine = true,
        cursorBrush = SolidColor(Classic.lcd),
        visualTransformation = if (hidden) PasswordVisualTransformation() else VisualTransformation.None,
        keyboardOptions = KeyboardOptions(
            autoCorrectEnabled = false,
            keyboardType = if (hidden) KeyboardType.Password else KeyboardType.Ascii,
            imeAction = if (next) ImeAction.Next else ImeAction.Done,
        ),
        keyboardActions = KeyboardActions(onDone = {
            focus.clearFocus()
            onDone()
        }),
        decorationBox = { field ->
            if (text.isEmpty()) BasicText("32 letters and digits", style = mono(15.sp, Classic.dim))
            field()
        },
    )
}

@Composable
private fun Steps(vararg steps: Pair<String, @Composable () -> Unit>) {
    Column(
        Modifier.padding(start = 12.dp, end = 12.dp, top = 6.dp, bottom = 10.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        for ((number, step) in steps) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                BasicText(number, style = mono(13.sp, Classic.dim))
                Column(Modifier.weight(1f)) { step() }
            }
        }
    }
}

@Composable
private fun Link(text: String, url: String) {
    val uri = LocalUriHandler.current
    BasicText(
        text,
        Modifier.clickable { uri.openUri(url) },
        body(13.sp, Classic.lcd, FontWeight.Normal).copy(lineHeight = 18.sp),
    )
}

/** Some text, then a value to copy. */
@Composable
private fun Copy(text: String, value: String) {
    val context = LocalContext.current
    var copied by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        BasicText(text, style = body(13.sp, Classic.dim, FontWeight.Normal))
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            BasicText(value, Modifier.weight(1f), mono(13.sp, Classic.list))
            TextButton(if (copied) "COPIED" else "COPY", {
                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                clipboard.setPrimaryClip(ClipData.newPlainText("Redirect URI", value))
                copied = true
            })
        }
    }
}

/** Spotify's client IDs and Last.fm's keys: 32 hexadecimal digits. */
private fun isKey(text: String) = text.length == 32 && text.all { it in '0'..'9' || it.lowercaseChar() in 'a'..'f' }

/** "2.2 MB", "1.4 GB". */
private fun size(bytes: Long): String {
    val mb = bytes / (1024.0 * 1024.0)
    return when {
        bytes == 0L -> "0 MB"
        mb >= 1024 -> "%.1f GB".format(Locale.ROOT, mb / 1024).replace(".0 ", " ")
        mb >= 10 -> "%.0f MB".format(Locale.ROOT, mb)
        else -> "%.1f MB".format(Locale.ROOT, mb)
    }
}
