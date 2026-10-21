package dev.spotless

import android.app.Application
import dev.spotless.core.Native

class SpotlessApp : Application() {
    /** The one core, for as long as the process lives. */
    lateinit var core: Core
        private set

    override fun onCreate() {
        super.onCreate()
        Native.init(this)
        core = Core(this)
    }
}
