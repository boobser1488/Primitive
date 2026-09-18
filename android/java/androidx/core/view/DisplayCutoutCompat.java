package androidx.core.view;

import android.os.Build;

import androidx.core.graphics.Insets;

/**
 * A display cutout, of which GameActivity asks exactly one question.
 *
 * The waterfall inset is how far the glass curves round the edge on a
 * phone whose screen does. {@code DisplayCutout.getWaterfallInsets}
 * arrived in API 30; before that the framework had no notion of it, so
 * the answer below 30 is zero rather than an estimate.
 */
public final class DisplayCutoutCompat {
    private final android.view.DisplayCutout cutout;

    DisplayCutoutCompat(android.view.DisplayCutout cutout) {
        this.cutout = cutout;
    }

    public Insets getWaterfallInsets() {
        if (Build.VERSION.SDK_INT < 30) {
            return Insets.NONE;
        }
        android.graphics.Insets got = cutout.getWaterfallInsets();
        return Insets.of(got.left, got.top, got.right, got.bottom);
    }
}
