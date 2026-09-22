package com.primitive.game;

import android.os.Build;
import android.view.Display;
import android.view.KeyEvent;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;

import com.google.androidgamesdk.GameActivity;

/**
 * GameActivity with the system bars out of the way.
 *
 * <h2>Why any Java of our own at all</h2>
 *
 * Because hiding the navigation bar is not a window flag, and window
 * flags were the only thing reachable from Rust.
 *
 * The game asked for {@code FULLSCREEN | LAYOUT_IN_SCREEN |
 * LAYOUT_NO_LIMITS} through {@code AndroidApp::set_window_flags} and
 * the bar stayed exactly where it was. Measured with
 * {@code adb shell dumpsys window} afterwards:
 *
 * <pre>
 * InsetsSource type=navigationBars frame=[0,1172][2712,1220] visible=true
 * cur=2712x1220 app=2608x1172
 * </pre>
 *
 * Those flags say where the window may be *laid out*; whether the bar
 * is *shown* is a different question, and on API 30 and up it is
 * answered by {@link WindowInsetsController}, which lives on a view and
 * must be spoken to on the UI thread. {@code android_main} runs on its
 * own thread, so Rust cannot call it without a way to hop threads --
 * which means a Java object either way. This is the smallest one that
 * does the job: an override of the hook GameActivity provides for
 * exactly this, called on the UI thread by construction.
 *
 * <h2>Why the bars are hidden rather than drawn under</h2>
 *
 * Drawing under them is the modern advice and it is advice for apps
 * with a scrolling list and a toolbar. This is a landscape game whose
 * interface reaches every corner: a translucent bar over the hotbar is
 * still a bar over the hotbar. Hidden, with the swipe that brings them
 * back left working, is what every game on the platform does.
 */
public class PrimitiveActivity extends GameActivity {

    /**
     * GameActivity's own hook for configuring the window, called before
     * the surface exists.
     *
     * The cutout mode goes here rather than in the theme because a
     * theme of our own would mean a {@code res/values} and an
     * {@code @style} reference for one attribute -- and this class had
     * to exist anyway.
     */
    @Override
    protected void onSetUpWindow() {
        super.onSetUpWindow();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            WindowManager.LayoutParams params = getWindow().getAttributes();
            params.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            getWindow().setAttributes(params);
        }
        askForTheFastestRefreshRate();
        hideTheSystemBars();
    }

    /**
     * Asks the platform to run the panel at its highest refresh rate.
     *
     * <h2>Why this is not the system's job</h2>
     *
     * **Measured on the device, and it is the whole explanation for a
     * frame rate that made no sense.** This phone's panel does 60, 90
     * and 120, and sits at 120 on the home screen. With the game
     * running under vsync, {@code dumpsys display} said:
     *
     * <pre>
     * mActiveSfDisplayMode=DisplayMode{id=0, ... refreshRate=60.000004}
     * renderFrameRate 60.000004
     * </pre>
     *
     * Android had dropped the panel to 60 for this app, because the app
     * never said otherwise. Everything else followed from that one
     * fact, and each step looked like a separate bug:
     *
     * <ul>
     * <li>a 60 Hz panel means FIFO can only ever hand over 60 frames a
     *     second, so "vsync on" read as 60 rather than 120;</li>
     * <li>at 60 the GPU has idle time, so the frequency governor drops
     *     its clock -- the *same* terrain pass measured 5.9 ms with
     *     vsync off and 9.7 ms with it on;</li>
     * <li>and a pass reading 9.7 ms looks exactly like a phone that
     *     cannot hold 120, which is the conclusion this replaces.</li>
     * </ul>
     *
     * With vsync off the panel stayed at 120, because a surface being
     * fed 123 frames a second is demand the governor can see. Asking is
     * how a game gets the same answer without burning the frames.
     *
     * <h2>Why the mode and not the rate</h2>
     *
     * {@code preferredRefreshRate} is older and softer: the platform
     * treats it as a hint and is free to ignore it.
     * {@code preferredDisplayModeId} names one of the modes the display
     * itself reported, which is a request the compositor honours. Only
     * modes at the current resolution are considered -- a mode is a
     * size *and* a rate, and a game asking for a faster panel has not
     * asked to be rendered at a different resolution.
     */
    private void askForTheFastestRefreshRate() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
            // `Display.Mode` arrived in API 23. Below it there is no
            // way to ask and no phone that needed asking.
            return;
        }
        Display display = getWindowManager().getDefaultDisplay();
        if (display == null) {
            return;
        }
        Display.Mode current = display.getMode();
        Display.Mode best = current;
        for (Display.Mode mode : display.getSupportedModes()) {
            if (mode.getPhysicalWidth() == current.getPhysicalWidth()
                && mode.getPhysicalHeight() == current.getPhysicalHeight()
                && mode.getRefreshRate() > best.getRefreshRate()) {
                best = mode;
            }
        }
        // **Set even when the display is already there, and that was
        // the bug in the first version of this.** The early return said
        // "already at the maximum, nothing to ask for" -- and at
        // `onSetUpWindow` time the panel *is* still at 120, because the
        // launcher was. The drop happens later, once the compositor has
        // watched a vsync-paced surface produce exactly 60 frames a
        // second and concluded that 60 is all this app wants. A request
        // is a standing statement about what the app needs; making it
        // conditional on the current state means never making it at the
        // one moment it would have mattered. Verified by its absence
        // from `dumpsys window`'s `mAttrs` for our window.
        WindowManager.LayoutParams params = getWindow().getAttributes();
        params.preferredDisplayModeId = best.getModeId();
        // The older, softer spelling alongside it. It is a hint the
        // platform may ignore, costs nothing when the line above is
        // honoured, and is the only one of the two that exists on the
        // phones where `preferredDisplayModeId` is filtered.
        params.preferredRefreshRate = best.getRefreshRate();
        getWindow().setAttributes(params);
        // One line, once per focus change, so that "did the request go
        // out" is answerable from `adb logcat` rather than by reasoning
        // about it -- which is how the first version was wrong for a
        // whole build cycle.
        android.util.Log.i("Primitive", "[display] asked for mode " + best.getModeId()
            + " at " + best.getRefreshRate() + " Hz (was mode " + current.getModeId()
            + " at " + current.getRefreshRate() + " Hz)");
    }

    /**
     * ...and again whenever the game comes back to the front.
     *
     * **The bars come back on their own and this is not optional.** A
     * swipe from the edge shows them, a notification shade pulled down
     * and released shows them, and returning from another app shows
     * them. Hidden once at startup means hidden until the first time
     * anything happens, which on a phone is about a minute.
     */
    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            // The refresh rate is re-asked for the same reason the bars
            // are re-hidden: coming back from another app is a fresh
            // negotiation, and a request made once at startup is a
            // request that lasts until the first notification shade.
            askForTheFastestRefreshRate();
            hideTheSystemBars();
        }
    }

    /**
     * Swallows a printable keystroke while the soft keyboard is up.
     *
     * <h2>The bug: every character arrived twice</h2>
     *
     * **Measured on the device, from the game's own trace.** Typing
     * {@code q}, {@code a}, {@code 1} into the name field produced:
     *
     * <pre>
     * [ime] editor "qq"     (was "")
     * [ime] editor "qqaa"   (was "qq")
     * [ime] editor "qqaa11" (was "qqaa")
     * </pre>
     *
     * with **no** {@code [ime] wrote} line anywhere — so the game never
     * echoed anything back. The doubling is already in GameTextInput's
     * own buffer before the game reads it. Backspace doubled too: the
     * same trace shows a name being erased two characters per tap.
     *
     * <h2>Why it happens, and why only some characters</h2>
     *
     * {@code gametextinput.InputConnection} is declared
     * {@code implements View.OnKeyListener} and carries {@code onKey}
     * *as well as* {@code commitText} and {@code sendKeyEvent}. One tap
     * on a soft key therefore has two roads into one {@code Editable}:
     * the key event, delivered to the view's key listener, and the
     * text commit, delivered through the input connection.
     *
     * That is also the whole of why it looked like a numeric-field
     * problem for so long. Cyrillic has no key code — there is no key
     * to send for `щ`, which is the reason this game runs on
     * GameActivity at all — so Russian only ever travelled the second
     * road and never doubled. Latin and digits have key codes and
     * travelled both.
     *
     * <h2>Why here and not in the library</h2>
     *
     * {@code android/games-activity-2.0.2-classes.jar} is Google's and
     * is shipped unmodified — see {@code README-games-activity.txt},
     * and the version is a chain rather than a preference. So the fix
     * has to be upstream of it. {@code Activity.onKeyDown} is too late:
     * a view's {@code OnKeyListener} runs during view dispatch, and the
     * activity only hears about keys no view consumed.
     * {@code dispatchKeyEvent} is the first thing in the window that
     * sees the event, which is the one place a key can be stopped
     * before the listener inserts it.
     *
     * <h2>Why only while the soft keyboard is showing</h2>
     *
     * Because that is exactly when the second road exists. With a
     * hardware keyboard and no soft input, the key event is the *only*
     * road, and swallowing it would lose the character instead of
     * de-duplicating it. The insets are the platform's own answer to
     * "is the keyboard up", so nothing here has to be kept in step by
     * hand.
     *
     * Navigation is untouched: Enter, Tab, the arrows and Back carry no
     * printable character and are not swallowed. They are how a form is
     * left and submitted, and the input method has no opinion about
     * them — the same division the Rust side already makes.
     */
    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        if (event != null && softKeyboardIsShowing() && isTextRatherThanNavigation(event)) {
            // Consumed, and reported as handled: the text is already on
            // its way through the input connection.
            return true;
        }
        return super.dispatchKeyEvent(event);
    }

    /**
     * Whether this keystroke is a character rather than a way around
     * the form.
     *
     * Delete counts: the trace showed it deleting two characters per
     * tap, for the same reason and by the same two roads.
     */
    private static boolean isTextRatherThanNavigation(KeyEvent event) {
        int code = event.getKeyCode();
        if (code == KeyEvent.KEYCODE_DEL || code == KeyEvent.KEYCODE_FORWARD_DEL) {
            return true;
        }
        if (code == KeyEvent.KEYCODE_ENTER
            || code == KeyEvent.KEYCODE_NUMPAD_ENTER
            || code == KeyEvent.KEYCODE_TAB
            || code == KeyEvent.KEYCODE_ESCAPE
            || code == KeyEvent.KEYCODE_BACK) {
            return false;
        }
        // A printable character, in the layout the player is actually
        // using -- `getUnicodeChar` applies the meta state, so it is the
        // letter that would be typed rather than the key that was hit.
        return event.getUnicodeChar() != 0;
    }

    private boolean softKeyboardIsShowing() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            // Below API 30 there is no way to ask, and no device this
            // has to run on. Leaving the keys alone is the safe answer:
            // a doubled character is worse than nothing only on the
            // phones that can be asked.
            return false;
        }
        View decor = getWindow().getDecorView();
        WindowInsets insets = decor.getRootWindowInsets();
        return insets != null && insets.isVisible(WindowInsets.Type.ime());
    }

    private void hideTheSystemBars() {
        View decor = getWindow().getDecorView();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            WindowInsetsController controller = decor.getWindowInsetsController();
            if (controller != null) {
                controller.hide(WindowInsets.Type.systemBars());
                // Transient rather than gone for good: a swipe from the
                // edge brings them back for a moment and they leave
                // again. Taking away the way out of a game is not the
                // game's decision to make.
                controller.setSystemBarsBehavior(
                    WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            }
        } else {
            // The pre-30 spelling of the same thing. `IMMERSIVE_STICKY`
            // is the half that makes the bars leave again by themselves
            // after a swipe; without it the first edge swipe undoes all
            // of this for the rest of the session.
            decor.setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
        }
    }
}
