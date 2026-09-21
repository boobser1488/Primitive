//! Sound: everything the player hears, and nothing they see.
//!
//! The fifth layer, and the last one to arrive. Like [`crate::engine`]
//! it is a device and a pipeline in front of it; unlike the engine it
//! has almost no state, because a sound is a thing that happens rather
//! than a thing that is.
//!
//! | module   | what it owns                                          |
//! |----------|-------------------------------------------------------|
//! | `clip`   | a decoded sound, and the random numbers that pick one  |
//! | `bank`   | every sound there is, and what material a block is     |
//! | `recorded` | which recording plays for which sound, and decoding   |
//! | `music`  | the composer, which writes as it plays                 |
//! | `mixer`  | the audio thread: voices, panning, the master bus      |
//! | `soundscape` | what the *game* sounds like: footsteps, ambience   |
//!
//! ## Recorded effects, generated music
//!
//! **Every effect is a recording, and nothing is generated.** This used
//! to say "nothing is recorded"; then the recordings arrived one by one
//! over recipes that stayed as the floor; then the player ordered every
//! sound to be a CC0 recording and every recipe deleted, and it was. What
//! plays is 3.4 MB of Vorbis compiled into the game (see `recorded`),
//! decoded once at startup. A sound with no recording is silent rather
//! than synthesised -- `recorded::SILENT` names the one there is and why.
//!
//! The music is still generated one bar at a time while it plays (see
//! `music`). The order was about the sounds, and a composer that writes
//! as it plays never repeats where a loop always does.
//!
//! `hound` is what keeps it an open box: `--export-sounds` writes every
//! recording as the game decodes it, and a sample of each of the moods,
//! out as `.wav`, and anything dropped into `assets/sounds` is played
//! instead of the clip of the same name. Same story as `embedded` tells
//! about the textures, and the same rule at the bottom of it: **what is
//! built in is the fallback, and a file on disk wins.**
//!
//! ## Two threads, and the line between them
//!
//! The game thread decides *what* is heard and *from where*; the audio
//! thread decides what the speaker does about it. They share atomics and
//! one small queue, and nothing else. See `mixer` for why that boundary
//! is drawn exactly where it is.
//!
//! ## Failure is silence
//!
//! A machine with no sound card, a device that disappears when a headset
//! is unplugged, a driver that refuses the format -- all of them produce
//! one line on the console and a game that runs. [`Audio::silent`] is a
//! real, working object whose methods do nothing, so there is no
//! `Option<Audio>` threaded through the frame loop and no way to forget
//! to check it.

pub mod bank;
pub mod mixer;
pub mod music;
pub mod recorded;
pub mod soundscape;
pub mod clip;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use glam::Vec3;

pub use bank::{Bank, Sfx};
pub use music::Mood;
pub use soundscape::Soundscape;

use mixer::{Mixer, Shared};

/// The sound layer, as the rest of the client sees it.
///
/// Cloneable-in-spirit but deliberately not `Clone`: the stream must be
/// dropped exactly once, on the thread that made it, and there is
/// exactly one of these.
pub struct Audio {
    /// `None` when there is no working device -- see the module note.
    shared: Option<Arc<Shared>>,
    /// Kept alive and otherwise untouched. Dropping it stops the device.
    _stream: Option<cpal::Stream>,
    /// What the device ended up running at, for the debug overlay.
    pub sample_rate: u32,
    pub channels: u16,
    /// What the device is called. Printed once at startup, which is how
    /// somebody with two sound cards finds out which one the game took.
    pub device_name: String,
    /// Every effect asked for, in order -- kept only under test, where the
    /// scenario runner (`crate::scenario`) asserts on what a player would
    /// have *heard*. A silent `Audio` has no mixer to ask, and a sound
    /// that is never requested is exactly the bug a scenario is for: the
    /// chest that opens without a creak because the arm that plays it was
    /// never reached.
    #[cfg(test)]
    pub heard: std::sync::Mutex<Vec<Sfx>>,
}

impl Audio {
    /// Opens the default output device and starts the mixer on it.
    ///
    /// Never fails: every error path ends in [`Audio::silent`] and a
    /// line on stderr. Sound is not worth refusing to start a game over,
    /// and a player whose headset is unplugged should get their world.
    pub fn start(assets_dir: &Path) -> Audio {
        match Self::try_start(assets_dir) {
            Ok(audio) => audio,
            Err(e) => {
                eprintln!("no sound: {e}");
                Audio::silent()
            }
        }
    }

    /// An `Audio` that does nothing, for a machine with no output.
    pub fn silent() -> Audio {
        Audio {
            shared: None,
            _stream: None,
            sample_rate: 0,
            channels: 0,
            device_name: "none".to_string(),
            #[cfg(test)]
            heard: Default::default(),
        }
    }

    fn try_start(assets_dir: &Path) -> Result<Audio, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no output device".to_string())?;
        let device_name = device.name().unwrap_or_else(|_| "unnamed".to_string());
        let supported = device
            .default_output_config()
            .map_err(|e| format!("no usable output format: {e}"))?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate.0;
        let channels = config.channels as usize;

        // Made for the device's rate, so every recording is resampled once
        // on the way in and never on the way to the speaker.
        let started = std::time::Instant::now();
        let mut bank = Bank::new(rate);
        bank.load_overrides(&assets_dir.join(bank::SOUNDS_DIR));
        let overridden = bank.overridden;
        let clips = bank.clip_count();
        let bytes = bank.bytes();
        let seconds = bank.seconds();

        let shared = Arc::new(Shared::new(Arc::new(bank)));

        // The recordings, on their own thread -- see `Bank::load_recordings`
        // for why they are not decoded here. If the thread cannot even be
        // started the game is silent apart from the music and a resource
        // pack's files, which is a quiet game and not a broken one.
        {
            let bank = shared.bank.clone();
            let assets_dir = assets_dir.to_path_buf();
            let spawned = std::thread::Builder::new()
                .name("sound recordings".to_string())
                .spawn(move || {
                    let started = std::time::Instant::now();
                    bank.load_recordings(&assets_dir);
                    // The sounds with nothing to play are named, so a player
                    // who misses one knows it is meant to be missing.
                    let silent: Vec<String> = recorded::SILENT.iter().map(|(sfx, _)| sfx.file_name()).collect();
                    println!(
                        "sound: {} of {} sounds recorded, decoded in {} ms; silent: {}",
                        bank.recorded(),
                        bank::all().len(),
                        started.elapsed().as_millis(),
                        silent.join(", ")
                    );
                });
            if let Err(e) = spawned {
                eprintln!("sound recordings not loaded: {e}");
            }
        }
        // Seeded from the rate and the channel count rather than from
        // the clock: two runs on the same machine hear the same first
        // piece, which is what makes "the music did something odd"
        // reportable.
        let mut mixer = Mixer::new(shared.clone(), rate as f32, (rate as u64) << 4 | channels as u64);

        let on_error = |e| eprintln!("sound device error: {e}");

        // The scratch buffer for the formats that are not `f32`. Sized
        // on the first callback and never again -- the device asks for
        // the same length every time.
        let mut scratch: Vec<f32> = Vec::new();

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config,
                move |data: &mut [f32], _| mixer.fill(data, channels),
                on_error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &config,
                move |data: &mut [i16], _| {
                    scratch.resize(data.len(), 0.0);
                    mixer.fill(&mut scratch, channels);
                    for (out, sample) in data.iter_mut().zip(&scratch) {
                        *out = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                },
                on_error,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                &config,
                move |data: &mut [u16], _| {
                    scratch.resize(data.len(), 0.0);
                    mixer.fill(&mut scratch, channels);
                    for (out, sample) in data.iter_mut().zip(&scratch) {
                        // Unsigned is signed with the zero point moved
                        // to the middle of the range.
                        let signed = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i32;
                        *out = (signed + i16::MAX as i32 + 1) as u16;
                    }
                },
                on_error,
                None,
            ),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| format!("could not open the output stream: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("could not start the output stream: {e}"))?;

        println!(
            "sound: {device_name}, {rate} Hz, {channels} ch -- \
{clips} pack clips ({seconds:.0} s, {} KiB) read in {} ms{}",
            bytes / 1024,
            started.elapsed().as_millis(),
            if overridden > 0 {
                format!(", {overridden} replaced from assets/sounds")
            } else {
                String::new()
            }
        );

        Ok(Audio {
            shared: Some(shared),
            _stream: Some(stream),
            sample_rate: rate,
            channels: config.channels,
            device_name,
            #[cfg(test)]
            heard: Default::default(),
        })
    }

    /// Is anything actually going to come out?
    pub fn is_running(&self) -> bool {
        self.shared.is_some()
    }

    /// Where the ears are. Called once a frame from the camera.
    pub fn set_listener(&self, position: glam::DVec3, right: Vec3) {
        if let Some(shared) = &self.shared {
            shared.set_listener(position, right);
        }
    }

    /// The two sliders on the settings screen, as fractions.
    ///
    /// **The effects bus is at full and follows the master.** It was
    /// pinned at zero while the game was music-only. The effects are back,
    /// and until the settings screen grows a slider of its own the master
    /// sets the effects and the music slider sets the music relative to
    /// it -- so footsteps without music is possible and music without
    /// footsteps is not, which is what a third slider is for.
    pub fn set_volumes(&self, master: f32, music: f32) {
        if let Some(shared) = &self.shared {
            shared.set_volumes(master, 1.0, music);
        }
    }

    /// Whether the player's head is under water, which muffles
    /// everything coming from the world.
    pub fn set_submerged(&self, submerged: bool) {
        if let Some(shared) = &self.shared {
            shared.set_submerged(submerged);
        }
    }

    /// What the music should be about. Cheap to call every frame.
    pub fn set_mood(&self, mood: Mood) {
        if let Some(shared) = &self.shared {
            shared.set_mood(mood);
        }
    }

    /// Plays a sound that is not anywhere: the interface, and anything
    /// that happens to the player themselves.
    ///
    /// These four are the one door every effect walks through. They were
    /// empty while the game was music-only -- the cut was made here rather
    /// than at the dozens of call sites -- and filling them is what brought
    /// every one of those call sites back at once.
    pub fn play(&self, sfx: Sfx) {
        self.play_flat(sfx, 1.0, 1.0);
    }

    /// The same, with a gain and a playback-rate multiplier.
    pub fn play_flat(&self, sfx: Sfx, gain: f32, pitch: f32) {
        #[cfg(test)]
        self.heard.lock().unwrap_or_else(|e| e.into_inner()).push(sfx);
        self.voice(sfx, gain, 0.0, pitch);
    }

    /// Plays a sound at a place in the world, placed against where the
    /// listener was last put -- see [`placement`].
    pub fn play_at(&self, sfx: Sfx, at: glam::DVec3, gain: f32, pitch: f32) {
        #[cfg(test)]
        self.heard.lock().unwrap_or_else(|e| e.into_inner()).push(sfx);
        let Some(shared) = &self.shared else {
            return;
        };
        let (ear, right) = shared.listener();
        let (distance_gain, pan) = placement((at - ear).as_vec3(), right);
        if distance_gain <= 0.0 {
            return;
        }
        self.voice(sfx, gain * distance_gain, pan, pitch);
    }

    /// The block at `cell`, heard from its middle rather than its
    /// corner.
    pub fn play_at_block(&self, sfx: Sfx, cell: (i32, i32, i32), gain: f32, pitch: f32) {
        let middle = glam::DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + 0.5, f64::from(cell.2) + 0.5);
        self.play_at(sfx, middle, gain, pitch);
    }

    /// Picks a variant, shakes it a little, and hands it to the mixer.
    ///
    /// **A small jitter on every sound, on top of whatever the caller
    /// asked for.** The soundscape already varies its footsteps; the
    /// interface, the chest and the eating did not, and five recordings
    /// of a click played at exactly the same pitch and level are five
    /// sounds the ear learns in a minute. Three per cent of pitch and
    /// eight of level is below what anybody hears as "a different sound"
    /// and above what they hear as "the same one".
    fn voice(&self, sfx: Sfx, gain: f32, pan: f32, pitch: f32) {
        let Some(shared) = &self.shared else {
            return;
        };
        let mut rng = shared.pick_rng();
        let Some(clip) = shared.bank.pick(sfx, &mut rng) else {
            return;
        };
        let pitch = pitch * (1.0 + 0.03 * rng.bipolar());
        let gain = gain * (1.0 + 0.08 * rng.bipolar());
        shared.push(mixer::Start { clip, gain, pan, pitch });
    }

    /// One line for the debug panel: what the game is playing through,
    /// and whether anything has been lost on the way.
    ///
    /// A dropped count above zero means the device stopped answering --
    /// which a player experiences as the sound simply going away, and
    /// which is otherwise invisible. It is the whole reason the counter
    /// exists.
    pub fn status(&self) -> String {
        if !self.is_running() {
            return "silent".to_string();
        }
        let dropped = self
            .shared
            .as_ref()
            .map(|s| s.dropped.load(Ordering::Relaxed))
            .unwrap_or(0);
        format!(
            "{} {} Hz x{}{}",
            self.device_name,
            self.sample_rate,
            self.channels,
            if dropped > 0 {
                format!("   {dropped} dropped")
            } else {
                String::new()
            }
        )
    }
}

/// How loud a sound at `offset` from the ears is, and where it sits
/// between the speakers.
///
/// **Full volume within [`NEAR`], falling as one over distance past it,
/// and gone at [`HEARING`].** Inverse distance rather than inverse square:
/// the square law is right for a point in a free field and wrong for a
/// game, where it makes a pick four blocks away as quiet as one through a
/// wall and leaves the world silent past eight. The linear taper on top
/// takes the last of it to zero at the edge instead of cutting off a
/// sound that is still audible, which is a click nobody can explain.
///
/// **Pan never reaches the end.** A sound exactly to the right is 80%
/// right, because a real one still reaches the far ear, and a hard-panned
/// footstep in headphones is a sound inside the head. And a sound at the
/// ears -- the block being dug, a step -- is centred, faded in over the
/// first block and a half, so digging straight down does not flick
/// between the speakers with every wobble of the camera.
pub fn placement(offset: Vec3, right: Vec3) -> (f32, f32) {
    let distance = offset.length();
    if distance >= HEARING {
        return (0.0, 0.0);
    }
    let falloff = NEAR / distance.max(NEAR);
    let taper = 1.0 - distance / HEARING;
    let gain = falloff * taper;
    let pan = if distance > 1e-3 {
        let side = offset.dot(right.normalize_or_zero()) / distance;
        side * 0.8 * (distance / 1.5).min(1.0)
    } else {
        0.0
    };
    (gain, pan.clamp(-1.0, 1.0))
}

/// Within this many blocks a sound is at its full level.
pub const NEAR: f32 = 3.0;

/// Past this many blocks nothing is heard: a fire at the edge of a
/// clearing, not one across the valley.
pub const HEARING: f32 = 40.0;

/// Writes every recording as the game decodes it, and a sample of each
/// mood, into `dir`.
///
/// The other half of the resource-pack story: what comes out here is
/// named exactly what the loader looks for, so the way to replace a
/// sound is to export, edit, and drop the file back in.
///
/// At 44.1 kHz rather than at whatever the machine's device runs at,
/// because this may well be run on a machine with no sound card at all,
/// and because 44.1 is what an audio editor expects.
pub fn export(dir: &Path, music_seconds: f32) -> std::io::Result<Vec<PathBuf>> {
    const RATE: u32 = 44_100;

    // The recordings are read from `assets` beside wherever this is run,
    // and from the copies compiled into the game where they are not there.
    let bank = Bank::new(RATE);
    bank.load_recordings(Path::new("assets"));
    let mut written = bank.export(dir)?;

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    for (name, mood) in [
        ("music.menu", Mood::Menu),
        ("music.day", Mood::Day),
        ("music.night", Mood::Night),
        ("music.cave", Mood::Cave),
        ("music.rain", Mood::Rain),
        ("music.storm", Mood::Storm),
        ("music.sea", Mood::Sea),
        ("music.forest", Mood::Forest),
        ("music.hearth", Mood::Hearth),
        ("music.peril", Mood::Peril),
    ] {
        let path = dir.join(format!("{name}.wav"));
        let rendered = music::Composer::render(mood, music_seconds, RATE, 0x_A110_C0DE);
        let mut writer = hound::WavWriter::create(&path, spec)
            .map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
        for sample in rendered {
            writer
                .write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
        }
        writer
            .finalize()
            .map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_further_away_is_never_louder() {
        let right = Vec3::X;
        let mut previous = f32::MAX;
        for step in 0..100 {
            let distance = step as f32 * 0.5;
            let (gain, _) = placement(Vec3::new(0.3, 0.1, distance), right);
            assert!(gain <= previous + 1e-6, "{distance} blocks was louder than the step before");
            previous = gain;
        }
    }

    #[test]
    fn a_sound_close_by_is_at_full_level_and_one_past_hearing_is_silent() {
        let (near, _) = placement(Vec3::new(0.0, -1.0, 1.0), Vec3::X);
        assert!(near > 0.9, "a block at the feet played at {near}");
        assert_eq!(placement(Vec3::new(0.0, 0.0, HEARING + 1.0), Vec3::X).0, 0.0);
    }

    #[test]
    fn a_sound_on_the_left_is_heard_on_the_left_but_never_only_there() {
        let right = Vec3::new(0.0, 0.0, 1.0);
        let (_, left) = placement(-right * 10.0, right);
        let (_, rightward) = placement(right * 10.0, right);
        assert!(left < -0.5 && left > -1.0, "left pan was {left}");
        assert!(rightward > 0.5 && rightward < 1.0, "right pan was {rightward}");
        let (_, ahead) = placement(Vec3::new(10.0, 0.0, 0.0), right);
        assert!(ahead.abs() < 1e-3, "straight ahead panned {ahead}");
    }

    #[test]
    fn the_block_being_dug_does_not_swing_between_the_speakers() {
        let (_, pan) = placement(Vec3::new(0.2, -0.5, 0.0), Vec3::X);
        assert!(pan.abs() < 0.15, "a block under the feet panned {pan}");
    }
}
