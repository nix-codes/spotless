package dev.spotless

import android.app.Application
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import com.github.takahirom.roborazzi.captureRoboImage
import dev.spotless.core.Action
import dev.spotless.core.demoArtist
import dev.spotless.ui.BrowseScreen
import dev.spotless.ui.PlaylistsScreen
import java.io.File
import org.junit.Assume
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.shadows.ShadowLooper

/**
 * The README's Android screenshots, drawn from the core's made-up library
 * (Core's demo) on the computer: `packaging/screenshots.sh` runs this.
 * A plain Application, not SpotlessApp, which would load the phone's
 * library.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], application = Application::class, qualifiers = "w411dp-h914dp-hdpi")
class ScreenshotTest {
    @get:Rule val compose = createComposeRule()

    private val folder = System.getProperty("spotless.screenshots")!!

    /** Skipped without the core built for this computer, which the phone's can't stand in for. */
    @Before
    fun needsCore() {
        val library = File(System.getProperty("jna.library.path")!!, System.mapLibraryName("spotless_ffi"))
        Assume.assumeTrue(
            "No ${library.path}: packaging/screenshots.sh builds it (cargo build --release -p spotless-ffi)",
            library.exists(),
        )
    }

    @Test
    fun playlists() = shoot("android.png") { core ->
        compose.setContent { PlaylistsScreen(core) {} }
    }

    @Test
    fun browse() = shoot("android-browse.png") { core ->
        core.apply(Action.OpenBrowse)
        core.apply(Action.OpenArtist(demoArtist()))
        compose.setContent { BrowseScreen(core) }
    }

    /**
     * Shows a screen of a demo core and draws it, with the clock stopped
     * so the song's name is drawn where its scrolling starts.
     */
    private fun shoot(file: String, show: (Core) -> Unit) {
        compose.mainClock.autoAdvance = false
        val core = Core(RuntimeEnvironment.getApplication(), demo = true)
        show(core)
        // The covers asked for come back through the main thread.
        repeat(3) {
            compose.mainClock.advanceTimeByFrame()
            ShadowLooper.idleMainLooper()
        }
        compose.onRoot().captureRoboImage("$folder/$file")
    }
}
