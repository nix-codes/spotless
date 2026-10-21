package dev.spotless.spike;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.SharedPreferences;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;

/** One screen: sign in, a track, album or artist link, Play, Pause, Resume, and the status. */
public final class SpikeActivity extends Activity {
    private static final String TRACK = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";

    private final Handler handler = new Handler(Looper.getMainLooper());
    private TextView status;

    private final Runnable poll = new Runnable() {
        @Override
        public void run() {
            status.setText(Native.status());
            String url = Native.takeUrl();
            if (url != null) {
                startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
            }
            handler.postDelayed(this, 500);
        }
    };

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        Native.init(getApplicationContext(), getFilesDir().getPath(), getCacheDir().getPath());
        if (Build.VERSION.SDK_INT >= 33) {
            requestPermissions(new String[] {Manifest.permission.POST_NOTIFICATIONS}, 0);
        }

        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        // Android 15 draws under the system bars: keep clear of them.
        layout.setOnApplyWindowInsetsListener((view, insets) -> {
            int top, bottom;
            if (Build.VERSION.SDK_INT >= 30) {
                android.graphics.Insets bars = insets.getInsets(
                    WindowInsets.Type.systemBars() | WindowInsets.Type.ime());
                top = bars.top;
                bottom = bars.bottom;
            } else {
                top = insets.getSystemWindowInsetTop();
                bottom = insets.getSystemWindowInsetBottom();
            }
            view.setPadding(48, top + 48, 48, bottom + 48);
            return insets;
        });
        status = new TextView(this);
        status.setTextSize(18);
        // The last link played, kept between starts.
        SharedPreferences prefs = getPreferences(MODE_PRIVATE);
        EditText track = new EditText(this);
        track.setText(prefs.getString("link", TRACK));
        track.setSingleLine();
        layout.addView(status);
        layout.addView(track);
        // The service keeps the process alive in the background: while the
        // browser is in front for the sign-in (else Android freezes the app
        // and the browser's redirect to the loopback port goes unanswered),
        // and while playing with the screen off.
        layout.addView(button("Sign in", () -> {
            startForegroundService(new Intent(this, SpikeService.class));
            Native.signIn();
        }));
        layout.addView(button("Play", () -> {
            startForegroundService(new Intent(this, SpikeService.class));
            String link = track.getText().toString();
            prefs.edit().putString("link", link).apply();
            Native.play(link);
        }));
        layout.addView(button("Pause", Native::pause));
        layout.addView(button("Resume", Native::resume));
        layout.addView(button("Stop the service", () ->
            stopService(new Intent(this, SpikeService.class))));
        setContentView(layout);
    }

    private Button button(String label, Runnable action) {
        Button button = new Button(this);
        button.setText(label);
        button.setOnClickListener(v -> action.run());
        return button;
    }

    @Override
    protected void onResume() {
        super.onResume();
        handler.post(poll);
    }

    @Override
    protected void onPause() {
        super.onPause();
        handler.removeCallbacks(poll);
    }
}
