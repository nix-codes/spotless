package dev.spotless.core

import android.content.Context

/** What the library needs before a [Spotless] is made (android/ffi/src/android.rs). */
object Native {
    init {
        System.loadLibrary("spotless_ffi")
    }

    /** Sets up the log, the certificates and the audio's context. Once, first. */
    @JvmStatic
    external fun init(context: Context)
}
