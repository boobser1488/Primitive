#!/usr/bin/env bash
#
# Builds an installable APK.
#
# ## Why not Gradle
#
# Gradle's job is to compile Java, resolve Maven dependencies and drive
# a plugin ecosystem. This package has almost none of those: it is one
# shared library, one manifest, one icon, a folder of PNGs -- and one
# Java class that is already compiled. What is left after Gradle's job
# is removed is the five commands below -- `aapt2 link`, `d8`, a zip,
# `zipalign`, `apksigner` -- all of which ship with the SDK this would
# need anyway. A Gradle project would add a wrapper, a daemon, two
# build files and a network fetch on a clean machine, to run the same
# five commands.
#
# The Java class is `com.google.androidgamesdk.GameActivity`, checked in
# as a jar of nine compiled classes and dexed here. It is not ours and
# there is no source tree for it; see
# `android/README-games-activity.txt` for where it came from, why the
# version is not a choice, and why the game cannot be typed in without
# it.
#
# ## What it produces
#
# `dist/primitive-<version>-android-arm64.apk`, signed with a debug key
# -- installable with `adb install` and by tapping it on a device with
# unknown sources allowed. It is NOT a release artefact: a debug key is
# a key everyone has, and the Play Store will not take it. Signing for
# release means a real keystore, which belongs to whoever ships the
# game and not in a repository.
#
# ## Usage
#
#   ./package-android.sh                 # release build, arm64
#   ./package-android.sh --debug         # faster build, unoptimised
#   ./package-android.sh --debuggable    # release, but `adb run-as` works
#   ./package-android.sh --install       # ...and push it to a device
#
set -euo pipefail

cd "$(dirname "$0")"

PROFILE="release"
CARGO_PROFILE_FLAG="--release"
INSTALL=0
DEBUGGABLE=0
for arg in "$@"; do
    case "$arg" in
        --debug) PROFILE="debug"; CARGO_PROFILE_FLAG="" ;;
        --debuggable) DEBUGGABLE=1 ;;
        --install) INSTALL=1 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

# --- the toolchain -----------------------------------------------------
#
# Found rather than assumed, and named in the error when it is not
# there: "command not found: aapt2" is a worse thing to read than a
# sentence saying which package to install.

: "${ANDROID_HOME:=${LOCALAPPDATA:-$HOME}/Android/Sdk}"
if [ ! -d "$ANDROID_HOME" ]; then
    echo "no Android SDK at $ANDROID_HOME" >&2
    echo "set ANDROID_HOME, or install the SDK through Android Studio." >&2
    exit 1
fi

# The newest of whatever is installed. Pinning a version here would mean
# editing this file every time the SDK updates.
BUILD_TOOLS_DIR="$(ls -d "$ANDROID_HOME"/build-tools/*/ 2>/dev/null | sort -V | tail -1)"
NDK_DIR="$(ls -d "$ANDROID_HOME"/ndk/*/ 2>/dev/null | sort -V | tail -1)"
PLATFORM_JAR="$(ls "$ANDROID_HOME"/platforms/*/android.jar 2>/dev/null | sort -V | tail -1)"

for found in "$BUILD_TOOLS_DIR:build-tools" "$NDK_DIR:ndk" "$PLATFORM_JAR:platforms"; do
    path="${found%:*}"
    what="${found##*:}"
    if [ -z "$path" ]; then
        echo "the SDK at $ANDROID_HOME has no $what installed" >&2
        echo "install it: sdkmanager \"$what;<version>\"" >&2
        exit 1
    fi
done

BUILD_TOOLS="${BUILD_TOOLS_DIR%/}"

# The SDK's tools are bare names on Linux and macOS, and some of them
# are `.bat` or `.exe` on Windows -- `apksigner` is a batch file, `aapt2`
# is an executable. Resolved by looking rather than by guessing from
# `uname`, because the same SDK is reached through a POSIX shell on
# Windows and both forms are then visible.
sdk_tool() {
    local name="$1"
    local candidate
    for candidate in "$BUILD_TOOLS/$name" "$BUILD_TOOLS/$name.exe" "$BUILD_TOOLS/$name.bat"; do
        if [ -f "$candidate" ]; then
            printf '%s' "$candidate"
            return 0
        fi
    done
    echo "the build-tools at $BUILD_TOOLS have no $name" >&2
    exit 1
}
AAPT2="$(sdk_tool aapt2)"
ZIPALIGN="$(sdk_tool zipalign)"
APKSIGNER="$(sdk_tool apksigner)"
# The dexer. New here, and the one thing in this script that exists
# because the game has a Java class in it -- exactly one, GameActivity,
# which is what gives the on-screen keyboard an `InputConnection` and
# therefore an alphabet larger than ASCII. See
# `android/README-games-activity.txt`.
D8="$(sdk_tool d8)"
NDK="${NDK_DIR%/}"
# The NDK ships one host toolchain per platform and names the directory
# after it.
NDK_HOST="$(ls -d "$NDK"/toolchains/llvm/prebuilt/*/ | head -1)"
NDK_BIN="${NDK_HOST%/}/bin"

# `.cargo/config.toml` names the linker as a bare command, so it has to
# be findable. See the note there about why it is not an absolute path.
export PATH="$NDK_BIN:$PATH"
export ANDROID_NDK_HOME="$NDK"

# cc-rs builds oboe's C++ for the same target, and needs to be told the
# API level too -- without one it uses libc++ headers that reach for
# symbols only API 30 has, and the build stops inside a standard header
# with no hint of why. The number must match `.cargo/config.toml` and
# `android/AndroidManifest.xml`.
API=24
# What the game is *built against*, as opposed to the oldest thing it
# runs on. Android reads this to decide which behaviours to apply: an
# app declaring an old target gets compatibility shims for rules made
# since, and the Play Store refuses uploads that declare one more than
# a year behind. 36 is Android 16.
TARGET_SDK=36
export CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android$API-clang"
export CXX_aarch64_linux_android="$NDK_BIN/aarch64-linux-android$API-clang++"
export AR_aarch64_linux_android="$NDK_BIN/llvm-ar"
# Windows hosts get the batch wrappers; everything else gets the real
# binaries. Checked rather than guessed from `uname`, because a Windows
# NDK inside a POSIX shell has both.
if [ -f "$CC_aarch64_linux_android.cmd" ]; then
    CC_aarch64_linux_android="$CC_aarch64_linux_android.cmd"
    CXX_aarch64_linux_android="$CXX_aarch64_linux_android.cmd"
    AR_aarch64_linux_android="$NDK_BIN/llvm-ar.exe"
    export CC_aarch64_linux_android CXX_aarch64_linux_android AR_aarch64_linux_android
fi

VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
TARGET="aarch64-linux-android"
ABI="arm64-v8a"
STAGE="target/android-package"
OUT="dist/primitive-$VERSION-android-arm64.apk"

echo "=== building the game for $TARGET ($PROFILE) ==="
cargo build -p primitive_client --lib --target "$TARGET" $CARGO_PROFILE_FLAG

SO="${CARGO_TARGET_DIR:-target}/$TARGET/$PROFILE/libprimitive.so"
[ -f "$SO" ] || { echo "cargo did not produce $SO" >&2; exit 1; }

# --- laying out the package -------------------------------------------

rm -rf "$STAGE"
mkdir -p "$STAGE/lib/$ABI" "$STAGE/assets" "$STAGE/res/mipmap"

# The library. Stripped in release: the debug build's symbols are about
# 190 MB of the 200 MB it produces, and an APK that size will not
# install on anything.
if [ "$PROFILE" = "release" ]; then
    "$NDK_BIN/llvm-strip" -o "$STAGE/lib/$ABI/libprimitive.so" "$SO" \
        2>/dev/null || cp "$SO" "$STAGE/lib/$ABI/libprimitive.so"
else
    cp "$SO" "$STAGE/lib/$ABI/libprimitive.so"
fi

# The C++ runtime, which is a separate file and has to travel with it.
#
# **Leaving this out is an APK that installs and then dies**, before a
# line of the game runs and with nothing on screen to say why -- the
# dynamic loader fails on `libc++_shared.so` and the activity is gone.
# It is easy to miss because nothing in the Rust build mentions C++ at
# all: the dependency arrives through `oboe`, which is the C++ library
# `cpal` reaches AAudio with, and which is deliberately built against
# the *shared* runtime rather than a static copy -- see the note beside
# `oboe-shared-stdcxx` in `primitive_client/Cargo.toml`. Two static
# copies of a C++ runtime in one process is a worse problem than an
# extra megabyte in the package.
#
# `readelf -d` on the built library lists it under NEEDED; this is the
# other half of that line.
CXX_RUNTIME="${NDK_HOST%/}/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so"
if [ ! -f "$CXX_RUNTIME" ]; then
    echo "the NDK at $NDK has no libc++_shared.so for $ABI" >&2
    echo "without it the package installs and then fails to load." >&2
    exit 1
fi
# Stripped like the game's own library. The NDK ships it with full
# debug information -- nine megabytes of it against about one of code --
# and none of that is any use inside a package on a phone.
if [ "$PROFILE" = "release" ]; then
    "$NDK_BIN/llvm-strip" -o "$STAGE/lib/$ABI/libc++_shared.so" "$CXX_RUNTIME"         2>/dev/null || cp "$CXX_RUNTIME" "$STAGE/lib/$ABI/"
else
    cp "$CXX_RUNTIME" "$STAGE/lib/$ABI/"
fi

# The list of files to unpack, and it is empty on purpose.
#
# This used to walk `assets/` and put every file in the package: 4 MB
# of pictures, recordings and models that `libprimitive.so` already
# carries, because `embedded.rs` compiles every one of them in. The APK
# held them twice (about 4 of its 10 MB), and the first launch unpacked
# a third copy into the app's data directory, where every file then
# *overrode* the built-in one it was identical to -- until an update
# changed a texture and the unpacked copy of the old one kept winning.
#
# The manifest itself stays, with nothing but a comment in it, rather
# than the unpack code going: a package without one is what the game
# refuses as "not built by this script", and the day a file has to live
# on disk rather than in the library (something too large to embed) it
# is one line here again. `platform::android::bootstrap` empties the
# old unpacked directory on the first launch of a new version, so an
# install that unpacked 1.5.0's copies does not keep them forever.
#
# Were a file ever listed here: only ASCII paths. `AAssetManager_open`
# takes a C string, and aapt2 refuses a non-ASCII path outright.
echo '# nothing to unpack: every asset is compiled into libprimitive.so' > "$STAGE/assets/MANIFEST"

# The launcher icon, scaled up from the game's own texture by the game
# itself -- see `--export-icon`.
cargo run -q -p primitive_client --bin primitive_client -- \
    --export-icon "$STAGE/res/mipmap/ic_launcher.png" 192

# --- building the APK --------------------------------------------------

mkdir -p dist
UNSIGNED="$STAGE/unsigned.apk"

# `aapt2 link` builds the APK from the manifest and the compiled
# resources. The game's own files and the shared library both go in
# afterwards, with `jar`.
#
# **Not `aapt2 -A` for the assets, and this one hides well.** On a
# Windows host aapt2 writes asset entries using the *host* path
# separator, so a file staged at `textures/animals/boar.png` lands in
# the package named `assets/textures` + backslash + `animals` +
# backslash + `boar.png`. The zip format says entry names use forward
# slashes, and Android believes it: `AAssetManager_open` asks for
# `textures/animals/boar.png`, finds nothing, and the game comes up with
# no textures at all.
#
# Nothing warns about it. The package installs, the app runs, and the
# only symptom is every asset missing at once -- on Windows only, so a
# build made on Linux would have looked fine.
"$AAPT2" compile --dir "$STAGE/res" -o "$STAGE/res.zip"
# `--debug-mode` on a debug build, and never on a release one.
#
# It sets `android:debuggable` in the packaged manifest, which is what
# `adb shell run-as` insists on before it will let anyone into the app's
# own directory -- and that directory is where the settings file, the
# saved worlds and `crash.log` are. Without it, the only account of what
# the game did on a phone is whatever it managed to say in logcat before
# it stopped saying anything.
#
# A release package must not carry it: debuggable means any process on
# the device may attach to this one and read everything it holds.
#
# Deliberately *not* tied to `--debug`. Measuring how the game performs
# on a phone needs an optimised build that can still be looked inside,
# and an unoptimised one measures nothing: `--debuggable` is therefore
# its own switch, and `--debuggable` on its own gives a release build
# with the door left open.
DEBUG_FLAG=""
if [ "$DEBUGGABLE" = "1" ]; then
    DEBUG_FLAG="--debug-mode"
    echo "=== packaging as DEBUGGABLE -- do not ship this one ==="
fi

# Unquoted on purpose: empty must expand to no argument at all, and
# "$DEBUG_FLAG" would hand aapt2 an empty string it does not understand.
# shellcheck disable=SC2086
"$AAPT2" link \
    -o "$UNSIGNED" \
    $DEBUG_FLAG \
    -I "$PLATFORM_JAR" \
    --manifest android/AndroidManifest.xml \
    --min-sdk-version "$API" \
    --target-sdk-version "$TARGET_SDK" \
    "$STAGE/res.zip"

# The Java half of GameActivity, dexed.
#
# `classes.dex` at the root of the package is where Android looks for an
# application's code, and the manifest's `hasCode="true"` is what makes
# it look at all. Both halves are needed: `hasCode="false"` with a dex
# present is a `ClassNotFoundException` on launch, and `hasCode="true"`
# with no dex is the same.
#
# `--min-api` must match the manifest's `minSdkVersion`. Told, not
# guessed: d8 defaults to 1 and then emits desugaring and multidex
# arrangements for platforms this package already declares it does not
# run on -- harmless but larger, and it hides a real mismatch behind a
# build that works anyway.
#
# `--lib` is the platform jar, which d8 needs to resolve the framework
# classes GameActivity extends. Without it every reference to
# `android.app.Activity` is unresolved and d8 stops.
#
# `--release` drops the debug line tables. There is no Java source in
# this repository to map them back to, so they would describe a file
# nobody has.
echo "=== dexing GameActivity ==="
GAMES_ACTIVITY_JAR="android/games-activity-2.0.2-classes.jar"
if [ ! -f "$GAMES_ACTIVITY_JAR" ]; then
    echo "missing $GAMES_ACTIVITY_JAR" >&2
    echo "see android/README-games-activity.txt for what it is and where it comes from." >&2
    exit 1
fi

# The AndroidX classes GameActivity is compiled against, replaced.
#
# **This is the one place this build has Java source in it**, and it is
# eight small files under `android/java`. GameActivity is declared
# `extends androidx.appcompat.app.AppCompatActivity` and calls a handful
# of `androidx.core.view` helpers, and its *native* half looks up
# `androidx/core/graphics/Insets` and `androidx/core/view/WindowInsetsCompat$Type`
# by name through JNI. Those classes have to exist in the package or the
# activity does not start -- the failure is
# `ClassNotFoundException: com.google.androidgamesdk.GameActivity`, with
# the real cause hidden in a suppressed exception underneath it.
#
# Taking the real AndroidX would mean taking AppCompat's *resources*:
# its delegate refuses to run against a theme that is not a
# `Theme.AppCompat` descendant. That means merging the `res/` of about
# twenty AARs, generating an `R` class per library package so their
# prebuilt bytecode resolves, and compiling those -- which is Gradle's
# job written out by hand, to obtain behaviour this game does not use.
# It draws its own interface into a surface: no menus, no dialogs, no
# toolbar, no night mode.
#
# So the shims. Each one forwards to the framework API the real class
# wraps, and says at its own site what it gives up. See
# `android/java/androidx/appcompat/app/AppCompatActivity.java` for the
# argument in full, and `android/README-games-activity.txt` for the
# version pinning that goes with it.
#
# `-source 8 -target 8` because that is what the checked-in
# `classes.jar` was compiled as, and d8 is happier merging one class
# file version than two. `-Xlint:-options` silences JDK 21's opinion
# about being asked for 8.
echo "=== compiling the AndroidX shims ==="
mkdir -p "$STAGE/javac"
find android/java -name '*.java' > "$STAGE/javac/sources.txt"
# **One** classpath entry, which is the reason this is safe to have at
# all. The shims themselves name nothing but the framework, but
# `PrimitiveActivity` extends `GameActivity`, so javac has to see
# Google's jar. A path *list* would have to be spelled with the host's
# separator -- `;` on Windows, `:` everywhere else -- which is the sort
# of thing that works on the machine it was written on; a single entry
# has no separator in it and travels.
#
# Our own sources all compile in this one invocation, so the AndroidX
# stubs that `GameActivity` refers to are found as siblings rather than
# through a second path.
javac -nowarn -Xlint:-options \
    -source 8 -target 8 \
    -bootclasspath "$PLATFORM_JAR" \
    -classpath "$GAMES_ACTIVITY_JAR" \
    -d "$STAGE/javac" \
    "@$STAGE/javac/sources.txt"

mkdir -p "$STAGE/dex"
# Both halves in one dex: Google's compiled classes and our shims. d8
# resolves the references between them here, so a shim that has drifted
# from what GameActivity expects is a build error rather than a phone
# that installs and dies.
"$D8" --release --min-api "$API" --lib "$PLATFORM_JAR" \
    --output "$STAGE/dex" \
    "$GAMES_ACTIVITY_JAR" \
    $(find "$STAGE/javac" -name '*.class')
[ -f "$STAGE/dex/classes.dex" ] || { echo "d8 produced no classes.dex" >&2; exit 1; }

# ...and into the package, through `jar` for the same reason as
# everything else below: forward slashes on every host.
jar --update --file "$UNSIGNED" --no-manifest -C "$STAGE/dex" "classes.dex"

# The library, added with `jar` rather than `zip`.
#
# Not a preference: `zip` is on neither a stock Windows nor a stock
# macOS machine, and `jar` is -- this script already needs a JDK for
# `keytool` and `apksigner`, so reaching for the JDK's own archiver
# costs no dependency at all.
#
# `-0` stores the library rather than deflating it. Android maps a
# stored library straight out of the package; a compressed one has to be
# unpacked to disk first, which doubles the installed size and is
# refused outright from API 23 up unless the manifest opts into it.
# `-M` keeps `jar` from writing a `META-INF/MANIFEST.MF`, which is a
# Java thing an APK has no use for and which would then have to come
# back out again before signing.
jar --update --file "$UNSIGNED" --no-compress --no-manifest -C "$STAGE" "lib"

# ...and the game's own files. Compressed as usual -- they are read once
# each, at unpack, so a smaller package is worth the moment it costs --
# and through `jar` for the reason above: it writes forward slashes on
# every host, because the JAR format has no host to be confused by.
jar --update --file "$UNSIGNED" --no-manifest -C "$STAGE" "assets"

# `-P 16` places the shared library on a 16 KB boundary inside the zip.
#
# Not the same thing as the library's *own* alignment, which the NDK
# already sets to 16 KB -- this is where the file starts in the package.
# Android maps a stored library straight out of the APK, so that offset
# has to be a multiple of the device's page size, and phones shipping
# with Android 15 and later use 16 KB pages. Aligned to 4 KB, as `-p`
# alone does, the library does not load on those at all: the app
# installs, and then dies in `dlopen` before a line of the game runs.
# `-P 16` rather than `-p`, and the two cannot be combined: `-p` means
# "page-align the library at 4 KB" and `-P` says which page size to use
# instead.
"$ZIPALIGN" -f -P 16 4 "$UNSIGNED" "$STAGE/aligned.apk"

# The debug key. Generated once and kept, so reinstalling over a
# previous build does not fail with a signature mismatch -- Android
# refuses an update signed by a different key, and a key regenerated
# every build is a different key every build.
#
# **Beside the repository, not inside `target/`.** It lived in `target/`
# and that quietly broke the promise in the paragraph above: `target/`
# is the directory every tool is allowed to delete. One `cargo clean`
# later the key was gone, the next build made a different one, and
# `adb install -r` answered
# `INSTALL_FAILED_UPDATE_INCOMPATIBLE: signatures do not match` -- with
# the only way out being to uninstall the app, which takes the player's
# settings and every saved world with it. A signing key is not a build
# artefact; it is the identity of the install, and it has to outlive the
# things that are safe to throw away.
#
# Not committed either -- see `.gitignore`. A key in a repository is a
# key everyone has.
KEYSTORE="android-debug.keystore"
if [ ! -f "$KEYSTORE" ]; then
    echo "=== making a debug signing key ==="
    keytool -genkeypair -v \
        -keystore "$KEYSTORE" \
        -storepass android -keypass android \
        -alias androiddebugkey \
        -keyalg RSA -keysize 2048 -validity 10000 \
        -dname "CN=Primitive Debug, OU=, O=, L=, S=, C=" >/dev/null
fi

"$APKSIGNER" sign \
    --ks "$KEYSTORE" \
    --ks-pass pass:android \
    --key-pass pass:android \
    --out "$OUT" \
    "$STAGE/aligned.apk"

"$APKSIGNER" verify "$OUT"

SIZE="$(du -h "$OUT" | cut -f1)"
echo
echo "=== $OUT ($SIZE) ==="
echo "install it with:  adb install -r $OUT"

if [ "$INSTALL" = "1" ]; then
    echo "=== installing ==="
    adb install -r "$OUT"
    # The component name changed with the activity class. `am start`
    # against `android.app.NativeActivity` now answers
    # `Activity class does not exist`, which reads like a broken install
    # rather than a stale command line.
    # **Our subclass, not Google's class.** The launch component moved
    # when `PrimitiveActivity` arrived to hide the system bars; aiming
    # `am start` at the old name answers "Activity class does not
    # exist", which reads like a broken install rather than a stale
    # command line. The same trap CLAUDE.md records from the move off
    # NativeActivity.
    adb shell am start -n com.primitive.game/com.primitive.game.PrimitiveActivity
fi
