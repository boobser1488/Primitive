package androidx.appcompat.app;

/**
 * The class GameActivity extends, and nothing else.
 *
 * <h2>Why this file exists</h2>
 *
 * {@code com.google.androidgamesdk.GameActivity} is declared as
 * {@code extends androidx.appcompat.app.AppCompatActivity}. Every call it
 * makes on that superclass is a lifecycle method that
 * {@link android.app.Activity} already has -- {@code onCreate},
 * {@code onResume}, {@code onTouchEvent}, {@code onTrimMemory} and eleven
 * others, all of them {@code super.} calls. It does not use an action bar,
 * a support fragment, a night-mode delegate or any other thing AppCompat
 * is for. The dependency is inherited scaffolding, not a feature.
 *
 * Taking the real AppCompat instead would mean taking its <em>resources</em>:
 * {@code AppCompatDelegate} refuses to start against anything that is not a
 * {@code Theme.AppCompat} descendant, and that theme is a resource, which
 * means merging the {@code res/} of about twenty AARs, generating an
 * {@code R} class per library package so their prebuilt bytecode resolves,
 * and compiling those. That is Gradle's job, done by hand, to obtain a
 * base class whose behaviour is not used. See
 * {@code android/README-games-activity.txt}.
 *
 * <h2>What is given up</h2>
 *
 * Everything AppCompat does, which for this application is nothing. The
 * game draws its own interface into a surface; it has no menus, no
 * dialogs, no toolbar and no views but the one GameActivity creates.
 *
 * <h2>How this fails, if it ever does</h2>
 *
 * Loudly and at build time. A future GameActivity that calls a method
 * only AppCompatActivity has does not compile against this: {@code d8}
 * reports the missing member while packaging, before anything is
 * installed. The dangerous version of this mistake -- a stub that
 * silently answers wrongly at runtime -- is not available here, because
 * this class answers nothing at all.
 */
public class AppCompatActivity extends android.app.Activity {
}
