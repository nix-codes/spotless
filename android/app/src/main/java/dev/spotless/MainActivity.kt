package dev.spotless

import android.Manifest
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import dev.spotless.core.Action
import dev.spotless.core.Session
import dev.spotless.core.spotifyLinkIn
import dev.spotless.ui.BrowseScreen
import dev.spotless.ui.PlaylistsScreen
import dev.spotless.ui.Scanner
import dev.spotless.ui.SettingsScreen
import dev.spotless.ui.SignInScreen

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (Build.VERSION.SDK_INT >= 33) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
        }
        val core = (application as SpotlessApp).core
        setContent { Screens(core) }
        // Not again when the activity is only made anew (turned, say).
        if (savedInstanceState == null) shared(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        shared(intent)
    }

    /** A picture of a Spotify Code, or a Spotify link, shared to Spotless: opened in Browse. */
    private fun shared(intent: Intent?) {
        if (intent?.action != Intent.ACTION_SEND) return
        val core = (application as SpotlessApp).core
        if (intent.type?.startsWith("image/") == true) {
            val picture = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                @Suppress("DEPRECATION")
                intent.getParcelableExtra(Intent.EXTRA_STREAM)
            }
            picture?.let(core::readCode)
        } else {
            val link = intent.getStringExtra(Intent.EXTRA_TEXT)?.let(::spotifyLinkIn) ?: return
            core.openShared(Action.OpenLink(link))
        }
    }

    override fun onStart() {
        super.onStart()
        (application as SpotlessApp).core.shown(true)
    }

    override fun onStop() {
        super.onStop()
        (application as SpotlessApp).core.shown(false)
    }
}

@Composable
private fun Screens(core: Core) {
    var settings by rememberSaveable { mutableStateOf(false) }
    when (core.session) {
        Session.SignedOut, Session.SigningIn, is Session.Failed -> SignInScreen(core)
        else -> if (core.scanning) {
            BackHandler { core.scanning = false }
            Scanner(core)
        } else if (core.browse.open) {
            BackHandler { core.browseBack() }
            BrowseScreen(core)
        } else if (settings) {
            BackHandler { settings = false }
            SettingsScreen(core) { settings = false }
        } else {
            PlaylistsScreen(core) { settings = true }
        }
    }
}
