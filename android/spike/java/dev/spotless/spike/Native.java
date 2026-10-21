package dev.spotless.spike;

import android.content.Context;

/** The Rust side (src/lib.rs). */
final class Native {
    static {
        System.loadLibrary("spotless_spike");
    }

    private Native() {}

    /** Once per process; later calls do nothing. */
    static native void init(Context context, String config, String cache);

    static native void signIn();

    static native void play(String uri);

    static native void pause();

    static native void resume();

    /** One line for the screen. */
    static native String status();

    /** The sign-in page to open, once; null when there is none. */
    static native String takeUrl();
}
