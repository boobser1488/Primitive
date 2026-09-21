package androidx.core.graphics;

/**
 * Four edge distances in pixels.
 *
 * <h2>Why the fields are public and named exactly this</h2>
 *
 * **The native half of GameActivity reads them by name through JNI.**
 * {@code GameActivity.cpp} does
 * {@code GET_FIELD_ID(gInsetsClassInfo.left, insets_class, "left", "I")}
 * for each of the four, having found the class as
 * {@code androidx/core/graphics/Insets}. Rename one, make one private, or
 * make one a {@code long}, and the JNI registration fails at activity
 * start with a message about a field, not about this file.
 *
 * That is also why there are no accessors: the real class has public
 * final int fields, and the C++ was written against them.
 */
public final class Insets {
    /** Read by JNI. See the class note. */
    public final int left;

    /** Read by JNI. See the class note. */
    public final int top;

    /** Read by JNI. See the class note. */
    public final int right;

    /** Read by JNI. See the class note. */
    public final int bottom;

    /**
     * All zeroes.
     *
     * A constant rather than a fresh object each time, because
     * GameActivity returns it for every inset type it has no answer for
     * and that is once per window-insets change.
     */
    public static final Insets NONE = new Insets(0, 0, 0, 0);

    private Insets(int left, int top, int right, int bottom) {
        this.left = left;
        this.top = top;
        this.right = right;
        this.bottom = bottom;
    }

    /** The four edges, or the shared {@link #NONE} when they are zero. */
    public static Insets of(int left, int top, int right, int bottom) {
        if (left == 0 && top == 0 && right == 0 && bottom == 0) {
            return NONE;
        }
        return new Insets(left, top, right, bottom);
    }
}
