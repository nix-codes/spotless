package dev.spotless

import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder

/**
 * While it runs, Android keeps the process, and the player in it, going.
 * Its notification is the media session's, from [Media], whose buttons
 * come back here.
 */
class PlaybackService : Service() {
    override fun onCreate() {
        super.onCreate()
        running = true
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val media = (application as SpotlessApp).core.media
        startForeground(NOTIFICATION, media.notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        media.pressed(intent?.action)
        return START_NOT_STICKY
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        (application as SpotlessApp).core.closed()
        super.onTaskRemoved(rootIntent)
    }

    override fun onDestroy() {
        running = false
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        const val NOTIFICATION = 1
        var running = false
            private set
    }
}
