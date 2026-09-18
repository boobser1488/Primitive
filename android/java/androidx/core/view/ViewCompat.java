package androidx.core.view;

import android.view.View;
import android.view.WindowInsets;

/**
 * The three static helpers GameActivity reaches for.
 *
 * All three exist on the framework's own {@link View} at the API levels
 * this package declares (24 and up), so each is a forward rather than a
 * reimplementation:
 *
 * <ul>
 *   <li>{@code generateViewId} -- API 17</li>
 *   <li>{@code getRootWindowInsets} -- API 23</li>
 *   <li>{@code setOnApplyWindowInsetsListener} -- API 20</li>
 * </ul>
 *
 * The real AndroidX class is not a forward: it keeps the listener in a
 * view tag so that it can be read back and chained, and that tag is
 * {@code androidx.core.R.id.tag_on_apply_window_listener} -- a
 * <em>resource</em>, and therefore a generated {@code R} class, and
 * therefore the resource-merging step this build does not have. See
 * {@code androidx.appcompat.app.AppCompatActivity}.
 *
 * Nothing here reads the listener back, so nothing here needs the tag.
 */
public final class ViewCompat {
    private ViewCompat() {
    }

    public static int generateViewId() {
        return View.generateViewId();
    }

    public static WindowInsetsCompat getRootWindowInsets(View view) {
        WindowInsets insets = view.getRootWindowInsets();
        return insets == null ? null : WindowInsetsCompat.toWindowInsetsCompat(insets);
    }

    /**
     * Installs a listener, wrapping each side in the other's type.
     *
     * The framework hands out an {@link WindowInsets} and wants one back;
     * the caller wants a {@link WindowInsetsCompat} and returns one. The
     * unwrap on the way out is why {@code WindowInsetsCompat} keeps the
     * framework object rather than copying the numbers out of it: a
     * listener that returned a *rebuilt* {@code WindowInsets} would drop
     * whatever fields it did not know to copy, and the view below would
     * be laid out against insets that lost their cutout.
     */
    public static void setOnApplyWindowInsetsListener(
            View view, final OnApplyWindowInsetsListener listener) {
        if (listener == null) {
            view.setOnApplyWindowInsetsListener(null);
            return;
        }
        view.setOnApplyWindowInsetsListener(new View.OnApplyWindowInsetsListener() {
            @Override
            public WindowInsets onApplyWindowInsets(View v, WindowInsets insets) {
                WindowInsetsCompat answer = listener.onApplyWindowInsets(
                        v, WindowInsetsCompat.toWindowInsetsCompat(insets));
                // A listener that answers with nothing is one that did
                // not want to change anything; the view gets what it was
                // given rather than a null it would crash on.
                if (answer == null || answer.toWindowInsets() == null) {
                    return insets;
                }
                return answer.toWindowInsets();
            }
        });
    }
}
