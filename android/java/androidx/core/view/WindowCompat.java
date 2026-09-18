package androidx.core.view;

import android.os.Build;
import android.view.View;
import android.view.Window;

/**
 * Whether the decor view leaves room for the system bars.
 *
 * GameTextInput's {@code InputConnection} turns this off so that the
 * keyboard's inset is reported to the application instead of being
 * silently subtracted from the content area.
 *
 * {@code Window.setDecorFitsSystemWindows} arrived in API 30. Below that
 * the equivalent is the older system-UI-visibility flags, which say the
 * same thing in the vocabulary of the day: lay out as if the bars were
 * not there. Written out rather than skipped, because a phone on API 24
 * that quietly did nothing here would keep the keyboard's height to
 * itself and the difference would show up as a text field under the
 * keyboard on old devices only.
 */
public final class WindowCompat {
    private WindowCompat() {
    }

    // `setSystemUiVisibility` is deprecated as of API 30, which is
    // exactly the level this branch is not taken at. Said here so the
    // build does not print a note about it on every run and train the
    // reader to ignore notes.
    @SuppressWarnings("deprecation")
    public static void setDecorFitsSystemWindows(Window window, boolean decorFitsSystemWindows) {
        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(decorFitsSystemWindows);
            return;
        }
        final int layoutStable = View.SYSTEM_UI_FLAG_LAYOUT_STABLE;
        final int layoutFullscreen = View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION;
        View decor = window.getDecorView();
        int flags = decor.getSystemUiVisibility();
        if (decorFitsSystemWindows) {
            flags &= ~(layoutStable | layoutFullscreen);
        } else {
            flags |= layoutStable | layoutFullscreen;
        }
        decor.setSystemUiVisibility(flags);
    }
}
