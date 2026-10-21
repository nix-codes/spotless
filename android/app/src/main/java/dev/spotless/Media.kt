package dev.spotless

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.drawable.Icon
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import dev.spotless.core.Action
import dev.spotless.core.Player
import dev.spotless.core.Status

/**
 * The player as the rest of Android sees it: the media session (the lock
 * screen's and the notification's controls, headset buttons), audio focus,
 * and pausing when headphones come out.
 */
class Media(private val context: Context, private val act: (Action) -> Unit) {
    private val main = Handler(Looper.getMainLooper())
    private val audio = context.getSystemService(AudioManager::class.java)
    private val notifications = context.getSystemService(NotificationManager::class.java)

    private var player: Player? = null
    private var art: ImageBitmap? = null
    private val status get() = player?.status ?: Status.STOPPED

    val session = MediaSession(context, "Spotless").apply {
        setCallback(object : MediaSession.Callback() {
            override fun onPlay() = play()
            override fun onPause() = pause()
            override fun onStop() = act(Action.Stop)
            override fun onSkipToNext() = act(Action.Next)
            override fun onSkipToPrevious() = act(Action.Previous)
            override fun onSeekTo(pos: Long) = act(Action.Seek(pos.coerceIn(0, UInt.MAX_VALUE.toLong()).toUInt()))
        })
    }

    /** Focus is held from playing until stopped, or until another app takes it. */
    private var focused = false
    /** Another app took focus for a while: play again when it's back. */
    private var resumeOnGain = false
    private val focus = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
        .setAudioAttributes(
            AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                .build(),
        )
        // Android lowers the sound for a moment itself: a notification
        // shouldn't pause the song.
        .setWillPauseWhenDucked(false)
        .setOnAudioFocusChangeListener({ change ->
            when (change) {
                AudioManager.AUDIOFOCUS_GAIN -> if (resumeOnGain) {
                    resumeOnGain = false
                    play()
                }
                AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> if (status == Status.PLAYING) {
                    resumeOnGain = true
                    pause()
                }
                AudioManager.AUDIOFOCUS_LOSS -> {
                    focused = false
                    resumeOnGain = false
                    pause()
                }
            }
        }, main)
        .build()

    /** Headphones came out: pause before the speaker plays it to the room. */
    private val noisy = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) pause()
        }
    }
    private var listening = false

    init {
        notifications.createNotificationChannel(
            NotificationChannel(CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW),
        )
    }

    /** Takes in the player's news, and its song's cover once read. */
    fun update(now: Player, cover: ImageBitmap?) {
        val was = player
        player = now
        val track = now.track
        val newArt = cover != art
        if (track?.uri != was?.track?.uri || newArt) {
            art = cover
            session.setMetadata(
                track?.let {
                    MediaMetadata.Builder()
                        .putString(MediaMetadata.METADATA_KEY_TITLE, it.title)
                        .putString(MediaMetadata.METADATA_KEY_ARTIST, it.artists)
                        .putString(MediaMetadata.METADATA_KEY_ALBUM, it.album)
                        .putLong(MediaMetadata.METADATA_KEY_DURATION, it.durationMs.toLong())
                        .apply { cover?.let { art -> putBitmap(MediaMetadata.METADATA_KEY_ART, art.asAndroidBitmap()) } }
                        .build()
                },
            )
        }
        session.setPlaybackState(
            PlaybackState.Builder()
                .setState(state(now.status), now.positionMs.toLong(), if (now.status == Status.PLAYING) 1f else 0f, SystemClock.elapsedRealtime())
                .setActions(
                    PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE or
                        PlaybackState.ACTION_STOP or PlaybackState.ACTION_SKIP_TO_NEXT or
                        PlaybackState.ACTION_SKIP_TO_PREVIOUS or PlaybackState.ACTION_SEEK_TO,
                )
                .build(),
        )
        session.isActive = now.status != Status.STOPPED
        if (now.status == Status.PLAYING && !focused) {
            focused = audio.requestAudioFocus(focus) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
            // A call is on: it goes first.
            if (!focused) pause()
        } else if (now.status == Status.STOPPED && focused) {
            audio.abandonAudioFocusRequest(focus)
            focused = false
            resumeOnGain = false
        }
        listen(now.status == Status.PLAYING || now.status == Status.LOADING)
        if (PlaybackService.running && (now.status != was?.status || track != was.track || newArt)) {
            notifications.notify(PlaybackService.NOTIFICATION, notification())
        }
    }

    /** The playback service's notification: the song, with its controls. */
    fun notification(): Notification {
        val track = player?.track
        val open = PendingIntent.getActivity(
            context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
        val playing = status == Status.PLAYING || status == Status.LOADING
        val actions = listOf(
            button(android.R.drawable.ic_media_previous, "Previous", PREVIOUS),
            if (playing) {
                button(android.R.drawable.ic_media_pause, "Pause", PAUSE)
            } else {
                button(android.R.drawable.ic_media_play, "Play", PLAY)
            },
            button(android.R.drawable.ic_media_next, "Next", NEXT),
        )
        return Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(track?.title ?: "Spotless")
            .setContentText(track?.artists)
            .apply { art?.let { setLargeIcon(it.asAndroidBitmap()) } }
            .setContentIntent(open)
            .setOngoing(true)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setStyle(Notification.MediaStyle().setMediaSession(session.sessionToken).setShowActionsInCompactView(0, 1, 2))
            .apply { actions.forEach(::addAction) }
            .build()
    }

    /** A notification button's press, through the service. */
    fun pressed(action: String?) {
        when (action) {
            PLAY -> play()
            PAUSE -> pause()
            NEXT -> act(Action.Next)
            PREVIOUS -> act(Action.Previous)
        }
    }

    private fun play() {
        act(if (status == Status.PAUSED) Action.TogglePause else Action.Play)
    }

    private fun pause() {
        if (status == Status.PLAYING || status == Status.LOADING) act(Action.TogglePause)
    }

    private fun listen(on: Boolean) {
        if (on == listening) return
        listening = on
        if (on) {
            val filter = IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY)
            if (Build.VERSION.SDK_INT >= 33) {
                context.registerReceiver(noisy, filter, Context.RECEIVER_NOT_EXPORTED)
            } else {
                context.registerReceiver(noisy, filter)
            }
        } else {
            context.unregisterReceiver(noisy)
        }
    }

    private fun button(icon: Int, title: String, action: String): Notification.Action {
        val intent = Intent(context, PlaybackService::class.java).setAction(action)
        val pending = PendingIntent.getService(context, action.hashCode(), intent, PendingIntent.FLAG_IMMUTABLE)
        return Notification.Action.Builder(Icon.createWithResource(context, icon), title, pending).build()
    }

    private fun state(status: Status) = when (status) {
        Status.STOPPED -> PlaybackState.STATE_STOPPED
        Status.LOADING -> PlaybackState.STATE_BUFFERING
        Status.PLAYING -> PlaybackState.STATE_PLAYING
        Status.PAUSED -> PlaybackState.STATE_PAUSED
    }

    companion object {
        const val CHANNEL = "playback"
        private const val PLAY = "dev.spotless.PLAY"
        private const val PAUSE = "dev.spotless.PAUSE"
        private const val NEXT = "dev.spotless.NEXT"
        private const val PREVIOUS = "dev.spotless.PREVIOUS"
    }
}
