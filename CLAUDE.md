# Primitive

A voxel survival game in Rust. ~280k lines, four crates, no engine underneath
it — the renderer, the mesher, the lighting, the physics, the sound and the
world generator are all in this repository and all readable.

This file is what a newcomer needs before touching anything. It is short on
purpose; the code carries the rest, and it carries it deliberately (see
**How this codebase is written**).

## The game

Stone age to iron age, alone or on a small server. A player wakes with
nothing, breaks a rock with their hands, and works up through flint, clay,
copper, bronze and iron. Every step is a *reason to go somewhere* rather than
a number going up: you leave the meadow because the ore is in the hills, and
you come back because the tannery is at home.

The design principle that decides arguments: **a mechanic should create a
decision, not a chore.** Wool is the warmest clothing and useless in rain, so
crossing the north is a plan rather than a stat. A boar is slower than a
running player, so meeting one is a choice to fight or leave. If a proposed
feature has one correct answer, it is not a feature yet.

## The crates

| crate | what it is |
|---|---|
| `primitive_shared` | The rules. Blocks, crafting, worldgen, lighting, inventory, animals, body, equipment. No I/O, no graphics. Both the client and the server depend on it, which is what stops them disagreeing. |
| `primitive_server` | Authority. Owns the world, validates every player action, streams chunks. Runs in-process for singleplayer and standalone for multiplayer — the *same* code, which is why singleplayer cannot drift from multiplayer. |
| `primitive_client` | Window, renderer, interface, sound, prediction. Talks to a server even when that server is inside it. |
| `primitive_modapi` | The native mod boundary. `mods/greeter` and `mods/flight` are in the workspace so a change that breaks a mod breaks the build. |

## Architecture worth knowing before you edit

**The client is never the authority.** It predicts, the server decides, and a
disagreement is a correction the client applies. Anything that changes what is
*true* about the world belongs in `primitive_server` or `primitive_shared`.

**The platform layer.** `primitive_client/src/platform/` is the line between
the game and whatever is holding the window. The game speaks
`platform::{Event, Key, Size, Window}`; `platform::winit_backend` is the only
file that names winit. Android is the same backend with different behaviour —
touch instead of a mouse, a surface that goes away when the activity does.
Do not reach for winit outside that directory.

**The frame waits first.** `GraphicsState::acquire()` blocks for a swapchain
image at the *top* of the frame, before the mouse is read. Waiting at the end
— which is where it used to be — means everything drawn was decided a whole
frame before it appeared. Do not move it back.

**Streaming is rationed.** Chunk integration, mesh dispatch and mesh upload
each get a slice of the frame (`streaming_budget`). An unbudgeted phase is a
phase that lands forty chunks in one frame and stutters. Results are handled
nearest-the-player first so the chunk someone just edited is never starved.

**The interface is laid out in its own space**: y in [-1, 1], x in
[-aspect, aspect]. `widgets::Layout` decides what grows with the size setting
and what stays put. **Anything that hit-tests must be the exact inverse of
what draws it**, and there are tests that say so — get this wrong and the
interface looks right and stops responding where it is drawn.

## Hard limits that bite

- **2048 texture layers, in one array or in several.** A layer is one number
  everywhere above the fragment shader, and a vertex carries eleven bits of
  it: eight at `mesh::LAYER_SHIFT`, the ninth at `LAYER_HIGH_SHIFT` in the
  hole the texture coordinate left, and the tenth and eleventh at
  `LAYER_TOP_SHIFT` in the `uv` word (a fine coordinate gave up a bit an axis
  for them — 63 pictures wide is still more than any face). The vertex is
  still 20 bytes. It was 256 once, when two ceilings happened to agree —
  eight bits and `Limits::default()` — and then 512; the atlas reached both.
  `blocks.toml` entries naming the same file still share a layer, and twelve
  garments still share four pictures through `types::garment_tint` — room is
  not a reason to stop. There is a test that fails the build when the atlas is
  full, and it explains the ways out.
- **A GLES driver only has to offer 256 layers an array, and the atlas is
  past it** (522 now). Vulkan and D3D12 guarantee 2048, and there the atlas
  is one array and every shader is the text in its file. Where the device
  offers less, `texture::AtlasSplit` lays the atlas across up to eight arrays
  of what it offers, bound side by side in group 1, and
  `AtlasSplit::specialise` rewrites every `textureSample*(block_textures, …)`
  into a function that picks the array by `layer / per_array` — one fetch, no
  extra draw calls. Anything that builds a pipeline over the atlas must take
  its layout from `split.layout_entries()`, its bind group from
  `bind_entries`, and its WGSL through `specialise`, or it works on a desktop
  and draws the wrong picture on a phone. `atlas_split_repro` draws a scene
  split at 256 beside the same scene whole; they must match to the byte.
  Eight arrays plus the terrain's three other textures is eleven of the
  sixteen texture units GLES promises a fragment stage.
- **Asset paths in an APK must be ASCII**, because `AAssetManager_open` takes
  a C string. The packaging script skips anything else, loudly.
- **Chunk coordinates are i32 and the render origin moves in whole blocks**,
  so no GPU coordinate is ever a seven-digit number.

## How this codebase is written

This is the part that surprises people, and it is not negotiable.

**Every non-obvious decision carries a comment saying *why*, and naming the
failure it prevents.** Not what the code does — the code says that. The
comment exists so the next person does not "simplify" it back into the bug it
was written to fix. Read `engine/renderer.rs` or `shared/animals.rs` for the
register: prose, specific, often naming the symptom a player saw.

A comment that restates the code is worse than no comment. A comment that says
"this used to do X, which broke Y" is the most valuable line in the file.

**Tests are named as full sentences stating the property**, not
`test_foo_works`:

```
fn running_at_a_flat_wall_from_every_angle_is_just_a_stop()
fn a_sheep_cannot_outrun_a_walking_player()
fn the_wheel_never_leaves_the_bar()
fn a_bigger_interface_is_still_clicked_where_it_is_drawn()
```

A test's name should tell you what broke when it goes red.

**`cargo clippy --workspace --all-targets` is silent and every test passes.**
That standard has held for the whole history of this repository. A warning is
not "just a warning" here.

**Design arguments belong in the code.** When you pick between two approaches,
write down the one you rejected and why. Several files carry a three-way
comparison in a doc comment; that is the house style, not decoration.

## Working on it

```
cargo clippy --workspace --all-targets      # must be silent
cargo test --workspace                      # ~2700 tests
cargo build --release -p primitive_client
cargo test -p primitive_client --lib scenario::   # the game, played by a script
```

**Every release must pass the scenarios.** `primitive_client/src/scenario/`
starts a real in-process server with the anticheat on, connects the real
client state (chunks, physics, mining, screens, mesher) without a window,
and plays what a player does -- climbs a staircase, walks into a rack, swims
out of a pool, works an anvil, dies in a rucksack -- asserting on what a
player would see and feel. They exist because every unit test passed while
the player found a staircase that lifted a whole block and stakes that
slowed nobody. A new mechanic gets a scenario; a bug a player finds gets one
first. `PRIMITIVE_SCENARIO_SHOTS=<absolute dir>` writes PNGs of what a
scenario looked at through the real renderer.

**A scenario's time is its frame counter, and the server's ticks come
from it** (`RunOptions::ticks_by_hand`). That is not a detail: the server
used to tick on its own 20 Hz wall clock while the client counted frames,
so on a machine with a few builds on it the world aged three times as
fast as the player did and one to five scenarios out of seventy went red
per run, never the same ones. The only thing still measured by the wall
is the *floor* under a frame -- never shorter than a sixtieth of a second
-- because the anticheat bills a player by `Instant::now()` and a harness
running faster than the wall would be corrected for speed. Two scenarios
want the wall on purpose (the phone in a pocket, the computer asleep) and
say so.

**Measuring, not guessing.** The game benchmarks itself:

```
PRIMITIVE_AUTOSTART=<world in saves/>   open straight into a world
PRIMITIVE_BENCH=<seconds>               run, then exit
PRIMITIVE_SHOT=<path> PRIMITIVE_SHOT_AFTER=<seconds>    write a PNG and carry on
PRIMITIVE_AUTOCONNECT=<host:port>       join a server straight away
PRIMITIVE_TEST_SPAWN=<x>,<z>            put a new player of any world there
                                        instead of the plaza or the spawn search
PRIMITIVE_DEBUG_PANEL=0                 keep the F3 panel off the screen (the
                                        console line still prints)
PRIMITIVE_GRAIN=<blocks>                how tall the landforms' fine relief
                                        stands; 0 draws the ground as it was
                                        before it had any, which is the
                                        "before" of a before-and-after
PRIMITIVE_IME_TYPE=<text>               type <text> into a new world's name,
                                        the way an input method commits it,
                                        and create the world
```

**The frame-cost switches** (`engine/opt.rs`), one per hypothesis, every one
off by default, so that a run with it and a run without it differ in exactly
one thing. They exist because a phone cannot be driven: a setting that can
only be reached through a menu cannot be measured on a device unattended.

```
PRIMITIVE_OPT_DEPTH_DISCARD=1     the main pass stops storing its depth
                                  buffer — nothing reads it, and on a
                                  tile-based GPU the store is a copy of the
                                  whole buffer out of tile memory each frame
PRIMITIVE_OPT_DEPTH_PREPASS=1     the solid terrain lays depth down first
                                  (no varyings, no fetch, no colour) and
                                  then shades with `LessEqual`. Multi-draw
                                  devices only; not while shadows are on
PRIMITIVE_OPT_TRILINEAR=1         at anisotropy 1, minification and the mip
                                  choice filter while magnification stays
                                  nearest — the pixel art is still crisp up
                                  close and the ground running away from the
                                  eye stops being one point sample of one
                                  over-coarse level
PRIMITIVE_OPT_MIP_BIAS=-0.5       the terrain fetch asks for a sharper mip
                                  level; compiled into the shader, so it
                                  costs nothing per fragment
PRIMITIVE_OPT_ANISO=4             overrides the anisotropy setting
PRIMITIVE_OPT_RESOLUTION=50       overrides the resolution scale, in per cent
PRIMITIVE_OPT_LOD=4               overrides where the coarse bands start, in
                                  chunks; 0 is the simplification off
```

The last three change nothing about how the game draws. They are there
because "is this pass paying for pixels or for triangles?" is answered by
halving the pixels and reading the stage line, and then by halving the
triangles and reading it again — the measurement `engine/lod.rs` is built on,
which until now could only be made on a desktop.

A build with any of them on prints one `[opt]` line at startup, so a
measurement taken off a device says on its face which build it came from.

`PRIMITIVE_IME_TYPE` is the Android text path checked without a person: it
hands the platform's editor a string exactly as GameTextInput would and then
the game's own mirror has to notice it, filter it and fill the field. On a
device the variables go in `primitive.env` in the app's own directory —
`adb shell run-as com.primitive.game` — because an activity started by the
system has an environment nobody chose.

These are not conveniences. **A phone cannot be driven**: MIUI refuses
synthetic input outright, so every menu tap on a device under test is a tap
a human has to make. Anything that has to be verified on Android has to be
reachable from the environment, or it cannot be verified unattended at all.
When something is not reachable that way, the fix is another hook, not a
hand-run experiment nobody can repeat.

With `debug_overlay_on_start = true` it prints one `[F3]` line per second:
fps, frame avg/p95/p99, per-pass GPU times, draw calls, culled chunks, and
where the frame's own milliseconds went. **`vsync = true` caps the frame rate
and hides everything — turn it off while measuring and put it back.**

`ui/snapshot.rs` renders interface screens without a GPU, which is how a
layout change is checked without a phone in your hand.

No change to performance lands without a before-and-after from the same
conditions. "It should be faster" is not a measurement.

## Android

```
./package-android.sh                # release APK in dist/
./package-android.sh --debuggable   # optimised, but `adb run-as` works
```

No Gradle: `aapt2`, `javac`, `d8`, `jar`, `zipalign`, `apksigner`. The game is
a `cdylib` loaded by **`GameActivity`**; `android_main` is in `lib.rs`.

There is Java in the package now, and it is the smallest amount that would
do. It used to be `NativeActivity` and no Java at all, and that could not be
typed in: a NativeActivity's content view has no `InputConnection`, so the
input method describes the editor as `TYPE_NULL` and sends it *keycodes* —
and there is no keycode for `щ`, in a game that ships in Russian with a font
declaring three alphabets. `GameActivity` carries GameTextInput and the text
arrives whole. What that costs is `android/games-activity-2.0.2-classes.jar`
(Google's, unmodified) and eight small shim classes under `android/java`,
dexed together by `d8`. Read `android/README-games-activity.txt` before
touching any of it — especially the part about the version not being a
choice.

Things learned the hard way, all of them documented at their site:

- Assets go in with `jar`, never `aapt2 -A` — aapt2 on Windows writes
  backslashes into entry names and then nothing loads.
- `libc++_shared.so` must be shipped; `oboe` needs it and its absence is an
  app that installs and dies in `dlopen`.
- The wgpu surface cannot exist before Android has a window: the loop is
  pumped until `Resumed` first, and therefore pumped for the rest of its life.
- `Suspended` is the last warning there is. The world is saved there.
- winit 0.29's Android `set_ime_allowed` is an empty function **and its
  `KeyEvent.text` is always `None`**. Neither matters any more and both are
  still true: the keyboard is raised with `AndroidApp::show_soft_input`, and
  the text comes from `AndroidApp::text_input_state` — the input method's own
  copy of the field, polled once a frame while a field has focus. See
  `platform::Window::ime_owns_text` for why a phone has two copies of one
  text field and how they are kept in step.
- **The GameActivity versions are a chain, not a preference.** winit 0.29
  depends on `android-activity` 0.5, which vendors GameActivity 2.0.2, which
  is the `.aar` whose classes have to be in the package. Mixing versions
  builds, installs, launches, and dies in `RegisterNatives`.
- **`hasCode="true"` and the dex are one thing.** Either without the other is
  `ClassNotFoundException` on launch — and the real cause hides in a
  *suppressed* exception under it, which is where an afternoon went.
- The launch component changed with the activity:
  `com.primitive.game/com.google.androidgamesdk.GameActivity`. `am start`
  against `android.app.NativeActivity` now says the class does not exist,
  which reads like a broken install rather than a stale command line.
- **The event loop must block when there is no surface.** Pumping with a
  zero timeout in every state burns a whole core in the player's pocket
  (measured: 99.6% of one, sustained). It needs both the pump timeout and
  `ControlFlow` changed together, because winit takes the smaller of the two.
- **Startup must service the loop.** `android_main` doing the asset unpack,
  `GraphicsState::new` and the sound bank before the loop is ever pumped
  means Android's lifecycle commands go unanswered, the Java thread blocks
  in `onPause`, and the watchdog kills the process at five seconds. It cost
  seven ANRs in one evening.
- **The signing key lives at `android-debug.keystore` beside the repo, never
  in `target/`.** A `cargo clean` used to change the identity of every
  build, and then `adb install -r` fails with
  `INSTALL_FAILED_UPDATE_INCOMPATIBLE` — the only way out being an uninstall
  that takes the player's worlds with it. A key is not a build artefact.
- `adb shell run-as` needs a package built `--debuggable`, and that is the
  only way to read the settings file, the saves or `crash.log` off a device.

The game's stdout reaches `adb logcat -s Primitive:*`; without that
redirection an Android build says nothing at all about why it failed.

## Where things are

```
primitive_shared/src/     blocks.rs crafting.rs worldgen.rs lighting.rs
                          animals.rs body.rs equipment.rs inventory.rs types.rs
primitive_client/src/
  engine/                 renderer.rs mesh.rs mesher.rs texture.rs *.wgsl
  logic/                  physics.rs chunk_manager.rs entities.rs animal_model.rs
  ui/                     widgets.rs menu.rs hud.rs inventory_screen.rs lang.rs
  platform/               mod.rs winit_backend.rs android.rs touch.rs
  audio/                  bank.rs music.rs      (sound is generated, not sampled)
primitive_server/src/     lib.rs logic/ net/
assets/textures/          blocks.toml plus the pictures
```

`CHANGELOG.md` is written by hand, in prose, and explains mechanisms rather
than listing changes. It is the best history of *why* this code looks like
this. `GUIDE.md` is the player-facing manual.
