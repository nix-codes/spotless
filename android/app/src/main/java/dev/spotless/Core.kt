package dev.spotless

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.util.Log
import java.util.concurrent.atomic.AtomicBoolean
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import dev.spotless.core.Action
import dev.spotless.core.Host
import dev.spotless.core.Message
import dev.spotless.core.PlaylistView
import dev.spotless.core.Session
import dev.spotless.core.Spotless
import dev.spotless.core.Status

/**
 * The Rust core and what the screens show of it. Everything here runs on
 * the main thread; the core wakes it from its own.
 */
class Core(private val context: Context) : Host {
    private val main = Handler(Looper.getMainLooper())

    /**
     * An approval page was opened and the app hasn't been back in front
     * since: Android would cut its network meanwhile (see [keepAlive]).
     */
    private var approving = false

    /** An update is posted, so wakes meanwhile need no other. */
    private val woken = AtomicBoolean(false)
    private val update = Runnable {
        woken.set(false)
        refresh()
    }
    /** The clock, while a song plays. */
    private val tick = object : Runnable {
        override fun run() {
            player = spotless.player()
            if (player.status == Status.PLAYING) main.postDelayed(this, 500)
        }
    }

    /** Stops the service once it has been unneeded for a while ([keepAlive]). */
    private val release = Runnable { service(start = false) }

    // After what wake() uses: the core's thread may call it before the
    // constructor is done.
    private val spotless = Spotless(context.filesDir.path, context.cacheDir.path, this)

    var session by mutableStateOf<Session>(Session.Connecting)
        private set
    var player by mutableStateOf(spotless.player())
        private set
    var tabs by mutableStateOf(spotless.tabs())
        private set
    /** The playlist the tabs show. */
    var playlist by mutableStateOf<PlaylistView?>(null)
        private set
    var browse by mutableStateOf(spotless.browse())
        private set
    var message by mutableStateOf<Message?>(null)
        private set

    init {
        refresh()
    }

    fun apply(action: Action) {
        spotless.apply(action)
        refresh()
    }

    /** The app is in front, or (false) in the background, where it is saved. */
    fun shown(inFront: Boolean) {
        if (inFront) {
            approving = false
            refresh()
        } else {
            spotless.save()
        }
    }

    override fun wake() {
        if (woken.compareAndSet(false, true)) main.post(update)
    }

    override fun open(url: String) {
        main.post {
            approving = true
            keepAlive()
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(intent)
        }
    }

    private fun refresh() {
        val saveIn = spotless.update()
        session = spotless.session()
        player = spotless.player()
        tabs = spotless.tabs()
        playlist = spotless.playlist(tabs.viewed)
        browse = spotless.browse()
        message = spotless.message()
        main.removeCallbacks(tick)
        if (player.status == Status.PLAYING) main.postDelayed(tick, 500)
        if (saveIn != null) {
            main.removeCallbacks(update)
            main.postDelayed(update, saveIn.toLong())
        }
        keepAlive()
    }

    /**
     * Android freezes an app in the background within seconds: while the
     * browser is in front for a sign-in, the frozen app never answers its
     * redirect, and with the screen off it loses the network mid-song. It
     * also closes the app's connections about five seconds in: a session
     * made in the browser's time would drop before the app is back. The
     * service keeps it going while any of that matters, and while a song
     * is loaded, paused too.
     *
     * It stops only once nothing has needed it for a while: Android doesn't
     * let an app in the background start it again, and a song skipped with
     * the screen off leaves the player stopped for a moment.
     */
    private fun keepAlive() {
        val needed = approving || session == Session.SigningIn || player.status != Status.STOPPED
        if (needed) {
            main.removeCallbacks(release)
            if (!PlaybackService.running) service(start = true)
        } else if (PlaybackService.running && !main.hasCallbacks(release)) {
            main.postDelayed(release, 30_000)
        }
    }

    private fun service(start: Boolean) {
        val intent = Intent(context, PlaybackService::class.java)
        try {
            if (start) context.startForegroundService(intent) else context.stopService(intent)
        } catch (error: IllegalStateException) {
            // Not allowed from the background: the next action in front will.
            Log.w("spotless", "playback service: $error")
        }
    }
}
