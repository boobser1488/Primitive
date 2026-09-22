games-activity-2.0.2-classes.jar
================================

What it is
----------

The Java half of GameActivity, lifted unchanged out of Google's
`androidx.games:games-activity:2.0.2` release:

    https://maven.google.com/androidx/games/games-activity/2.0.2/games-activity-2.0.2.aar
    sha256(aar)      fa80a6392e3f7d7e379bbc1e8bf2b848ac08a6eada1bda82484ddc806dba536d
    sha256(this jar) 6adb0f511e1572b8b5e8e7549234c84025f9c470e918c5d10c3485752cca268c

Nine classes, 17 KB:

    com/google/androidgamesdk/GameActivity
    com/google/androidgamesdk/GameActivity$InputEnabledSurfaceView
    com/google/androidgamesdk/BuildConfig
    com/google/androidgamesdk/gametextinput/GameTextInput
    com/google/androidgamesdk/gametextinput/GameTextInput$Pair
    com/google/androidgamesdk/gametextinput/InputConnection
    com/google/androidgamesdk/gametextinput/Listener
    com/google/androidgamesdk/gametextinput/Settings
    com/google/androidgamesdk/gametextinput/State

`package-android.sh` runs `d8` over it and puts the resulting
`classes.dex` in the APK. Nothing else in this repository is Java, and
there is still no Gradle.

Why it is here at all
---------------------

The on-screen keyboard. See the long note at the top of
`AndroidManifest.xml`: a `NativeActivity`'s content view has no
`InputConnection`, so the input method sends it keycodes rather than
text, and there is no keycode for `щ`. GameActivity's view has one.

Why the jar and not the .aar
----------------------------

The `.aar` is 855 KB, of which 830 KB is `prefab/` -- prebuilt static
libraries and headers for the *native* half, in four ABIs. None of that
is used: the `android-activity` crate vendors the same C++ and compiles
it itself with `cc`, against the NDK this build already needs. Shipping
the other 830 KB in the repository to use 17 KB of it would be storing
a second copy of code that is already in `~/.cargo` -- and a copy that
could silently differ from the one actually linked.

Why 2.0.2 and not the newest
----------------------------

**The version is not a choice.** The C++ and the Java are one library
split across two languages; they call each other through JNI with
signatures fixed per release. `android-activity` vendors a specific
GameActivity and says which:

    android-activity 0.5.2  ->  GameActivity 2.0.2

and winit 0.29 -- what this client is built on -- depends on
`android-activity 0.5`. So the chain that pins this file is:

    winit 0.29  ->  android-activity 0.5.2  ->  games-activity 2.0.2

Mixing versions does not fail to build. It builds, installs, launches,
and dies in `RegisterNatives` or on the first text event, on a thread
whose stack means nothing to anyone reading logcat.

How to refresh it
-----------------

Only when winit moves, and then to whatever the new `android-activity`
vendors -- check `GAMEACTIVITY_MAJOR_VERSION` in

    ~/.cargo/registry/src/*/android-activity-<ver>/**/game-activity/GameActivity.h

then:

    curl -LO https://maven.google.com/androidx/games/games-activity/<ver>/games-activity-<ver>.aar
    unzip -p games-activity-<ver>.aar classes.jar > android/games-activity-<ver>-classes.jar

and update the name in `package-android.sh`, which names it once.

Licence
-------

Apache 2.0, as the AAR ships it.
