package androidx.core.view;

import android.os.Build;
import android.view.WindowInsets;

import androidx.core.graphics.Insets;

/**
 * A wrapper over {@link android.view.WindowInsets}.
 *
 * The real class exists because the framework's inset API changed shape
 * three times between API 20 and 30 and AndroidX hides that. This one
 * exists because GameActivity's signatures name it, and because
 * {@code GameActivity.cpp} looks up {@code WindowInsetsCompat$Type} by
 * that exact path to ask which bit each inset kind is. See
 * {@code androidx.appcompat.app.AppCompatActivity} for why the real one
 * is not here.
 */
public final class WindowInsetsCompat {
    /**
     * The framework object underneath, or null where there is none --
     * a view that is not attached has no insets to report.
     */
    private final WindowInsets insets;

    private WindowInsetsCompat(WindowInsets insets) {
        this.insets = insets;
    }

    /** Wraps a framework object, or nothing. */
    public static WindowInsetsCompat toWindowInsetsCompat(WindowInsets insets) {
        return new WindowInsetsCompat(insets);
    }

    /** The framework object back, which is what a framework listener must return. */
    public WindowInsets toWindowInsets() {
        return insets;
    }

    /**
     * The insets of one or more kinds, as a bitmask from {@link Type}.
     *
     * <h2>Why API 30 is a cliff here rather than a gradient</h2>
     *
     * {@code WindowInsets.getInsets(int)} and the whole
     * {@code WindowInsets.Type} vocabulary arrived in API 30. Below that
     * the framework has one lump -- "system window insets" -- which
     * cannot be asked about the keyboard separately from the navigation
     * bar, because before API 30 it did not distinguish them.
     *
     * So below 30 this answers the lump for the bar-shaped kinds and
     * zero for the rest, and that is the honest answer rather than a
     * guess. The game does not lay anything out against insets -- it is
     * fullscreen and draws its own interface -- so what this returns
     * reaches GameTextInput and stops. A version of this file that
     * invented an IME height on API 24 would be inventing it for nobody.
     */
    // `getSystemWindowInset*` is deprecated as of API 30, which is
    // exactly the level the branch that calls it is not taken at. Said
    // here so the build does not print a note on every run and train
    // the reader to ignore notes.
    @SuppressWarnings("deprecation")
    public Insets getInsets(int typeMask) {
        if (insets == null) {
            return Insets.NONE;
        }
        if (Build.VERSION.SDK_INT >= 30) {
            android.graphics.Insets got = insets.getInsets(typeMask);
            return Insets.of(got.left, got.top, got.right, got.bottom);
        }
        if ((typeMask & Type.BARS) == 0) {
            return Insets.NONE;
        }
        return Insets.of(
                insets.getSystemWindowInsetLeft(),
                insets.getSystemWindowInsetTop(),
                insets.getSystemWindowInsetRight(),
                insets.getSystemWindowInsetBottom());
    }

    /** The cutout, if this display has one and this Android knows about it. */
    public DisplayCutoutCompat getDisplayCutout() {
        if (insets == null || Build.VERSION.SDK_INT < 28) {
            return null;
        }
        android.view.DisplayCutout cutout = insets.getDisplayCutout();
        return cutout == null ? null : new DisplayCutoutCompat(cutout);
    }

    /**
     * Which kind of inset is being asked about.
     *
     * <h2>Why all nine, when GameActivity's Java calls one</h2>
     *
     * The Java only ever calls {@link #ime()}. The *native* side calls
     * all nine: `GameActivity.cpp` resolves them in a loop --
     * `captionBar`, `displayCutout`, `ime`, `mandatorySystemGestures`,
     * `navigationBars`, `statusBars`, `systemBars`, `systemGestures`,
     * `tappableElement`, in that order, matching its own
     * `GameCommonInsetsType` enum -- and `GET_STATIC_METHOD_ID` on a
     * name that is not here aborts the JNI registration, which is the
     * activity failing to start. The names and the `()I` shape are not
     * ours to choose.
     *
     * The values are the framework's own bits from API 30, spelled out
     * rather than read from {@code WindowInsets.Type} so that this file
     * compiles and behaves the same on a device that predates them.
     */
    public static final class Type {
        private Type() {
        }

        private static final int STATUS_BARS = 1;
        private static final int NAVIGATION_BARS = 1 << 1;
        private static final int CAPTION_BAR = 1 << 2;
        private static final int IME = 1 << 3;
        private static final int SYSTEM_GESTURES = 1 << 4;
        private static final int MANDATORY_SYSTEM_GESTURES = 1 << 5;
        private static final int TAPPABLE_ELEMENT = 1 << 6;
        private static final int DISPLAY_CUTOUT = 1 << 7;

        /** The kinds the pre-API-30 lump can stand in for. See {@code getInsets}. */
        static final int BARS = STATUS_BARS | NAVIGATION_BARS | CAPTION_BAR;

        public static int statusBars() {
            return STATUS_BARS;
        }

        public static int navigationBars() {
            return NAVIGATION_BARS;
        }

        public static int captionBar() {
            return CAPTION_BAR;
        }

        public static int ime() {
            return IME;
        }

        public static int systemGestures() {
            return SYSTEM_GESTURES;
        }

        public static int mandatorySystemGestures() {
            return MANDATORY_SYSTEM_GESTURES;
        }

        public static int tappableElement() {
            return TAPPABLE_ELEMENT;
        }

        public static int displayCutout() {
            return DISPLAY_CUTOUT;
        }

        public static int systemBars() {
            return BARS;
        }
    }
}
