package dev.spotless

import android.Manifest
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
import dev.spotless.ui.BrowseScreen
import dev.spotless.ui.PlaylistsScreen
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
        else -> if (core.browse.open) {
            BackHandler {
                core.apply(if (core.browse.canGoBack) Action.Back else Action.CloseBrowse)
            }
            BrowseScreen(core)
        } else if (settings) {
            BackHandler { settings = false }
            SettingsScreen(core) { settings = false }
        } else {
            PlaylistsScreen(core) { settings = true }
        }
    }
}
