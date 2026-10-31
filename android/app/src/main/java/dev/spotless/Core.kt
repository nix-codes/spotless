package dev.spotless

import android.content.Context
import android.graphics.Bitmap
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Network
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.util.Log
import java.util.concurrent.atomic.AtomicBoolean
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.runtime.setValue
import dev.spotless.core.Action
import dev.spotless.core.BrowseTab
import dev.spotless.core.Host
import dev.spotless.core.Message
import dev.spotless.core.PlaylistView
import dev.spotless.core.Session
import dev.spotless.core.Spotless
import dev.spotless.core.SpotifyView
import dev.spotless.core.Status
import dev.spotless.core.Track
import dev.spotless.ui.Classic

/**
 * The Rust core and what the screens show of it. Everything here runs on
 * the main thread; the core wakes it from its own. With [demo], the core
 * holds the made-up library the screenshots show, and connects to nothing.
 */
class Core(private val context: Context, demo: Boolean = false) : Host {
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
    /**
     * The app was swiped away with the song paused: the service stays off
     * until something plays, though the player takes a moment to stop.
     */
    private var closedPaused = false
    /** Lets the player go once it has been paused for long ([keepAlive]). */
    private val putAway = Runnable { apply(Action.Release) }

    // After what wake() uses: the core's thread may call it before the
    // constructor is done.
    private val spotless = if (demo) {
        Spotless.demo(context.filesDir.path, context.cacheDir.path, this)
    } else {
        Spotless(context.filesDir.path, context.cacheDir.path, this)
    }

    /** The media session, audio focus and the service's notification. */
    val media = Media(context, ::apply)

    var session by mutableStateOf<Session>(Session.Connecting)
        private set
    var player by mutableStateOf(spotless.player())
        private set
    var tabs by mutableStateOf(spotless.tabs())
        private set
    /** The playlist the tabs show. */
    var playlist by mutableStateOf<PlaylistView?>(null)
        private set
    /** The Spotify tab's queue, while the tab shows. */
    var spotify by mutableStateOf<SpotifyView?>(null)
        private set
    var browse by mutableStateOf(spotless.browse())
        private set
    var message by mutableStateOf<Message?>(null)
        private set
    var settings by mutableStateOf(spotless.settings())
        private set
    /** The error last tapped away: hidden until the next action, which may meet it again. */
    var dismissed by mutableStateOf<String?>(null)
    /** The playlist to bring the playing song into sight in, once it shows. */
    var jump by mutableStateOf<ULong?>(null)

    /**
     * Shows the playlist the song plays from, or Play would start while
     * stopped, placed as on opening it: a tap on the player's song.
     */
    fun jumpToPlaying() {
        val list = player.playlist?.takeIf { player.track != null } ?: return
        if (browse.open) apply(Action.CloseBrowse)
        apply(Action.SelectPlaylist(list))
        jump = list
    }

    /** The colours Settings offers. */
    val phosphors = dev.spotless.core.phosphors()
    val groupings = dev.spotless.core.groupings()
    private var coloured = false

    /**
     * YOUR SPOTIFY shows the open playlist's songs rather than the list of
     * playlists: one at a time, as a phone has room for.
     */
    var spotifySongs by mutableStateOf(false)

    /** Browse's Back: to the playlists list, the page before, or out of Browse. */
    fun browseBack() = when {
        browse.shown == BrowseTab.SPOTIFY && spotifySongs -> spotifySongs = false
        browse.shown == BrowseTab.SEARCH && browse.canGoBack -> apply(Action.Back)
        else -> apply(Action.CloseBrowse)
    }

    /** Where each Browse page was scrolled to, by [browseKey], kept while Browse is closed. */
    val browseScroll = HashMap<String, LazyListState>()

    /** How LAST.FM's songs are sorted, kept while Browse is closed. */
    var lovedOrder by mutableStateOf(dev.spotless.ui.LovedOrder.ARTIST)

    /** Covers read so far, by URL; ones asked for and not read yet are missing. */
    private val covers = mutableStateMapOf<String, ImageBitmap>()
    private val asked = HashSet<String>()

    init {
        refresh()
        // A song waiting for the connection plays as soon as there is one.
        context.getSystemService(ConnectivityManager::class.java)
            .registerDefaultNetworkCallback(
                object : ConnectivityManager.NetworkCallback() {
                    override fun onAvailable(network: Network) = apply(Action.NetworkBack)
                },
                main,
            )
    }

    /** Removes every song from this playlist, viewed or not. */
    fun clear(id: ULong) {
        val entries = spotless.playlist(id)?.songs?.map { it.entry }.orEmpty()
        if (entries.isNotEmpty()) apply(Action.Remove(id, entries))
    }

    /** The songs of this playlist, viewed or not. */
    fun tracks(id: ULong): List<Track> = spotless.playlist(id)?.songs?.map { it.track }.orEmpty()

    /** The cover at [url], once read: asking reads it. */
    fun cover(url: String?): ImageBitmap? {
        if (url == null) return null
        if (asked.add(url)) spotless.loadCover(url)
        return covers[url]
    }

    fun apply(action: Action) {
        dismissed = null
        spotless.apply(action)
        refresh()
    }

    /**
     * The player as this phone plays it: the Spotify tab's queue playing on
     * another device is no sound here, so it takes no audio focus, media
     * session or service from the app that may be playing it.
     */
    private val heard get() = if (player.here) player.status else Status.STOPPED

    /**
     * The app was swiped away. A song playing goes on; a paused one is let
     * go (Play later goes on where it was), and the service, its
     * notification and the media controls with it, so Android can end the
     * app and another player gets the headset's buttons.
     */
    fun closed() {
        if (heard == Status.PLAYING || heard == Status.LOADING) return
        closedPaused = true
        if (heard != Status.STOPPED) apply(Action.Release)
        spotless.save()
        main.removeCallbacks(release)
        service(start = false)
    }

    /** The app is in front, or (false) in the background, where it is saved. */
    fun shown(inFront: Boolean) {
        if (inFront) {
            approving = false
            // Asleep, Android cuts the app's network without the phone losing
            // its own, so no network comes back: a session dropped meanwhile
            // is replaced now, not after the rest of the wait between tries.
            spotless.apply(Action.NetworkBack)
            refresh()
        } else {
            spotless.save()
        }
    }

    override fun wake() {
        if (woken.compareAndSet(false, true)) main.post(update)
    }

    /**
     * Offers the Spotify links of these songs, albums, artists or
     * playlists to another app, as the Spotify app's Share does.
     */
    fun share(uris: List<String>) {
        val links = dev.spotless.core.spotifyLinks(uris)
        if (links.isEmpty()) return
        val send = Intent(Intent.ACTION_SEND)
            .setType("text/plain")
            .putExtra(Intent.EXTRA_TEXT, links)
        val chooser = Intent.createChooser(send, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(chooser)
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
        spotify = spotless.spotify()
        browse = spotless.browse()
        message = spotless.message()
        val was = settings.phosphor
        settings = spotless.settings()
        if (settings.phosphor != was || !coloured) {
            phosphors.firstOrNull { it.key == settings.phosphor }?.let(Classic::use)
            coloured = true
        }
        for (cover in spotless.takeCovers()) {
            val image = cover.image ?: continue
            val width = image.width.toInt()
            val height = image.height.toInt()
            // As colours, not bytes: how a bitmap lays out its bytes is the
            // device's (on a computer, as for the screenshots, it differs).
            val rgba = image.rgba
            val pixels = IntArray(width * height) { i ->
                val at = i * 4
                (rgba[at + 3].toInt() and 0xff shl 24) or
                    (rgba[at].toInt() and 0xff shl 16) or
                    (rgba[at + 1].toInt() and 0xff shl 8) or
                    (rgba[at + 2].toInt() and 0xff)
            }
            covers[cover.url] = Bitmap.createBitmap(pixels, width, height, Bitmap.Config.ARGB_8888)
                .asImageBitmap()
        }
        media.update(if (player.here) player else player.copy(status = Status.STOPPED), cover(player.track?.coverUrl))
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
     * the screen off leaves the player stopped for a moment. A song paused
     * for long is let go, so that the service, and the app, don't stay
     * forever.
     */
    private fun keepAlive() {
        if (heard != Status.PAUSED) {
            main.removeCallbacks(putAway)
        } else if (!main.hasCallbacks(putAway)) {
            main.postDelayed(putAway, PAUSED_FOR)
        }
        val playing = heard == Status.PLAYING || heard == Status.LOADING
        if (playing) closedPaused = false
        val needed = approving || session == Session.SigningIn ||
            (heard != Status.STOPPED && !closedPaused)
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

    companion object {
        /** How long a song stays paused before the player is let go. */
        private const val PAUSED_FOR = 10 * 60 * 1000L
    }
}
