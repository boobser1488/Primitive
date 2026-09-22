package androidx.annotation;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Marks something a shrinker must not remove.
 *
 * A no-op here: nothing shrinks this package -- there is no R8 pass and
 * no ProGuard configuration in this build, and the one dexed jar is
 * Google's, taken whole.
 *
 * It is present only because GameActivity's class file names it in its
 * annotation table. A missing annotation class is not an error at
 * runtime -- the verifier skips annotations it cannot resolve -- but it
 * is a warning out of {@code d8} on every build, and a warning that is
 * expected is a warning nobody reads.
 */
@Retention(RetentionPolicy.CLASS)
@Target({ElementType.TYPE, ElementType.FIELD, ElementType.METHOD, ElementType.CONSTRUCTOR})
public @interface Keep {
}
