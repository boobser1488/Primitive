package androidx.core.view;

import android.view.View;

/**
 * Told when a view's insets change.
 *
 * The framework has its own interface of the same shape
 * ({@link android.view.View.OnApplyWindowInsetsListener}); this one
 * exists because GameActivity and GameTextInput's InputConnection both
 * declare that they implement <em>this</em> one. {@link ViewCompat} is
 * what bridges the two.
 */
public interface OnApplyWindowInsetsListener {
    WindowInsetsCompat onApplyWindowInsets(View view, WindowInsetsCompat insets);
}
