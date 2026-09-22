//! The mixer: what the audio thread does, and the only place the two
//! threads meet.
//!
//! ## The contract with the audio thread
//!
//! The device callback runs on a thread with a hard deadline -- a few
//! milliseconds, every few milliseconds, forever. Missing it is not slow
//! audio, it is a click, and a click is more noticeable than almost any
//! visual glitch. So the callback is allowed to do exactly three things:
//! arithmetic, reading atomics, and taking one lock that the other side
//! never holds for more than a memcpy.
//!
//! It is **not** allowed to allocate, to block, to call into the
//! operating system, or to touch anything that could be paged out. Every
//! shape in this file follows from that:
//!
//! * Voices are a fixed array, filled and emptied, never grown.
//! * A voice holds an `Arc<Clip>`, so the clip cannot be freed under it
//!   -- and the bank holds one too, so dropping the `Arc` on the audio
//!   thread never actually frees anything.
//! * Volumes and the underwater flag are atomics, read once per buffer.
//! * New sounds arrive through a queue guarded by a `Mutex` that the
//!   callback takes with `try_lock`. If the game thread happens to be
//!   holding it, the callback simply does not drain this buffer and the
//!   sound starts five milliseconds later. Nobody has ever heard that;
//!   everybody would hear the alternative.
//!
//! ## Where a sound gets placed
//!
//! Distance and direction are worked out on the **game** thread, in
//! [`super::Audio::play_at`], and arrive here as a gain and a pan. That
//! is not laziness about doppler or moving sources -- it is that every
//! clip in the bank is under a second long, and a source that does not
//! move over that time is a source that does not need tracking.
//! The recordings bent that a little -- a frog's croak is a second, a
//! gust or a roll of thunder a few -- and the long ones are exactly the
//! ones that are either unplaced or come from something that sits still. Music
//! and the interface are unplaced by definition.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use super::bank::Bank;
use super::music::{Composer, Mood};
use super::clip::{Clip, Rng};

/// How many clips can sound at once.
///
/// Reached only by something pathological -- a chunk of gravel
/// collapsing next to a fire in a storm. Past it the quietest voice is
/// dropped, which is both the cheapest choice and the right one.
const MAX_VOICES: usize = 32;

/// How many un-started sounds may pile up before new ones are dropped.
///
/// Only reachable if the device has stopped calling back at all, in
/// which case the game should carry on and lose the sounds rather than
/// grow a queue for the rest of the session.
const MAX_QUEUED: usize = 64;

/// Cutoff of the muffling filter applied while the player's head is
/// under water.
const SUBMERGED_CUTOFF: f32 = 620.0;

/// One sound waiting to start.
///
/// Positioning is already done -- see the module note -- so this is a
/// clip and three numbers.
pub struct Start {
    pub clip: Arc<Clip>,
    pub gain: f32,
    /// -1 hard left, 0 centre, 1 hard right.
    pub pan: f32,
    /// Playback rate multiplier. The cheapest variation there is, and
    /// the one the ear is least able to spot as a repeat.
    pub pitch: f32,
}

/// State both threads can see.
///
/// Everything here is either an atomic or a mutex the callback only ever
/// `try_lock`s. Volumes are stored as the bits of an `f32` because
/// `AtomicF32` does not exist and a lock for three numbers read once per
/// buffer would be absurd.
pub struct Shared {
    pub bank: Arc<Bank>,
    master: AtomicU32,
    sfx: AtomicU32,
    music: AtomicU32,
    submerged: AtomicBool,
    mood: AtomicU8,
    queue: Mutex<Vec<Start>>,
    /// Bumped on every sound played, and used as the seed that picks a
    /// variant. A counter rather than a shared generator, because the
    /// game thread must not take a lock to make a footstep.
    picks: AtomicU64,
    /// Listener position, as `f64` bits, and its right-hand vector, as `f32`
    /// bits. Atomics rather than a mutex for the same reason as the volumes:
    /// written once a frame, read on every positional sound.
    ///
    /// **The position is `f64`** because a sound is placed by subtracting it
    /// from where the sound is, and two `f32`s ten million blocks out are each
    /// a whole block off: a footstep a block to the left could be heard on the
    /// right.
    listener_at: [AtomicU64; 3],
    listener: [AtomicU32; 3],
    /// How many sounds were thrown away because the queue was full.
    /// Shown in the debug overlay; if it is ever non-zero, audio has
    /// stopped rather than got busy.
    pub dropped: AtomicU32,
}

#[inline]
fn store_f32(slot: &AtomicU32, value: f32) {
    slot.store(value.to_bits(), Ordering::Relaxed);
}

#[inline]
fn load_f32(slot: &AtomicU32) -> f32 {
    f32::from_bits(slot.load(Ordering::Relaxed))
}

impl Shared {
    pub fn new(bank: Arc<Bank>) -> Shared {
        Shared {
            bank,
            master: AtomicU32::new(1.0f32.to_bits()),
            sfx: AtomicU32::new(1.0f32.to_bits()),
            music: AtomicU32::new(1.0f32.to_bits()),
            submerged: AtomicBool::new(false),
            mood: AtomicU8::new(mood_index(Mood::Menu)),
            queue: Mutex::new(Vec::with_capacity(MAX_QUEUED)),
            picks: AtomicU64::new(1),
            listener_at: Default::default(),
            listener: Default::default(),
            dropped: AtomicU32::new(0),
        }
    }

    pub fn set_volumes(&self, master: f32, sfx: f32, music: f32) {
        store_f32(&self.master, master.clamp(0.0, 1.0));
        store_f32(&self.sfx, sfx.clamp(0.0, 1.0));
        store_f32(&self.music, music.clamp(0.0, 1.0));
    }

    pub fn set_submerged(&self, submerged: bool) {
        self.submerged.store(submerged, Ordering::Relaxed);
    }

    pub fn set_mood(&self, mood: Mood) {
        self.mood.store(mood_index(mood), Ordering::Relaxed);
    }

    /// Where the ears are, and which way is right. The forward vector is
    /// not needed: panning only cares about the left-right axis, and
    /// front-back is not something two speakers can express anyway.
    pub fn set_listener(&self, position: glam::DVec3, right: glam::Vec3) {
        for (slot, value) in self.listener_at.iter().zip(position.to_array()) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        store_f32(&self.listener[0], right.x);
        store_f32(&self.listener[1], right.y);
        store_f32(&self.listener[2], right.z);
    }

    pub fn listener(&self) -> (glam::DVec3, glam::Vec3) {
        let at = |axis: usize| f64::from_bits(self.listener_at[axis].load(Ordering::Relaxed));
        (
            glam::DVec3::new(at(0), at(1), at(2)),
            glam::Vec3::new(
                load_f32(&self.listener[0]),
                load_f32(&self.listener[1]),
                load_f32(&self.listener[2]),
            ),
        )
    }

    /// A fresh generator for picking one of a sound's variants.
    pub fn pick_rng(&self) -> Rng {
        Rng::new(self.picks.fetch_add(1, Ordering::Relaxed))
    }

    /// Hands a sound to the audio thread.
    ///
    /// Silently drops it if the queue has backed up -- see [`MAX_QUEUED`].
    pub fn push(&self, start: Start) {
        let Ok(mut queue) = self.queue.lock() else {
            // Poisoned means the audio thread panicked while holding it,
            // which it cannot -- there is no `unwrap` inside the lock.
            // If it somehow happens, losing sounds is the right failure.
            return;
        };
        if queue.len() >= MAX_QUEUED {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        queue.push(start);
    }
}

/// A mood as one byte, because that is what an atomic can hold and the
/// game thread and the audio thread do not share anything bigger.
///
/// The two functions are each other's inverse and
/// `every_mood_survives_the_trip_to_the_audio_thread` says so: a mood
/// added to one and forgotten in the other would arrive as the menu's
/// music, which is the sort of thing that gets shipped.
fn mood_index(mood: Mood) -> u8 {
    match mood {
        Mood::Menu => 0,
        Mood::Day => 1,
        Mood::Night => 2,
        Mood::Cave => 3,
        Mood::Rain => 4,
        Mood::Peril => 5,
        Mood::Storm => 6,
        Mood::Sea => 7,
        Mood::Forest => 8,
        Mood::Hearth => 9,
    }
}

fn mood_of(index: u8) -> Mood {
    match index {
        1 => Mood::Day,
        2 => Mood::Night,
        3 => Mood::Cave,
        4 => Mood::Rain,
        5 => Mood::Peril,
        6 => Mood::Storm,
        7 => Mood::Sea,
        8 => Mood::Forest,
        9 => Mood::Hearth,
        _ => Mood::Menu,
    }
}

/// One clip being played.
struct Voice {
    clip: Arc<Clip>,
    /// Fractional read position, so pitch can be anything rather than a
    /// whole number of samples per step.
    at: f64,
    step: f64,
    left: f32,
    right: f32,
}

impl Voice {
    /// One sample, linearly interpolated, or `None` when it has run out.
    #[inline]
    fn next(&mut self) -> Option<(f32, f32)> {
        let index = self.at as usize;
        let samples = &self.clip.samples;
        if index + 1 >= samples.len() {
            return None;
        }
        let frac = (self.at - index as f64) as f32;
        let a = samples[index] as f32;
        let b = samples[index + 1] as f32;
        // The clips are `i16`; this is the one place that scale is
        // undone.
        let sample = (a + (b - a) * frac) * (1.0 / i16::MAX as f32);
        self.at += self.step;
        Some((sample * self.left, sample * self.right))
    }

    /// How loud this voice is overall, for the stealing rule.
    #[inline]
    fn loudness(&self) -> f32 {
        self.left.max(self.right)
    }
}

/// Everything the callback owns.
pub struct Mixer {
    shared: Arc<Shared>,
    voices: Vec<Voice>,
    music: Composer,
    /// State of the underwater filter, per channel, and how much of it
    /// is currently mixed in. Ramped rather than switched: a filter that
    /// appears between one buffer and the next is a click.
    low_left: f32,
    low_right: f32,
    submerged: f32,
    /// One-pole coefficient for [`SUBMERGED_CUTOFF`] at this rate.
    alpha: f32,
    rate: f32,
}

impl Mixer {
    pub fn new(shared: Arc<Shared>, rate: f32, seed: u64) -> Mixer {
        let dt = 1.0 / rate;
        let rc = 1.0 / (std::f32::consts::TAU * SUBMERGED_CUTOFF);
        Mixer {
            shared: shared.clone(),
            voices: Vec::with_capacity(MAX_VOICES),
            music: Composer::new(rate, seed),
            low_left: 0.0,
            low_right: 0.0,
            submerged: 0.0,
            alpha: dt / (rc + dt),
            rate,
        }
    }

    /// Fills one buffer of interleaved frames.
    ///
    /// `channels` is whatever the device asked for: mono gets the sum,
    /// stereo gets the two sides, and anything with more channels gets
    /// the stereo pair in the first two and silence in the rest --
    /// which is wrong for a 5.1 setup and right for the far more common
    /// case of a device that reports four channels and uses two.
    pub fn fill(&mut self, out: &mut [f32], channels: usize) {
        self.drain_queue();

        let master = load_f32(&self.shared.master);
        let sfx_gain = load_f32(&self.shared.sfx) * master;
        let music_gain = load_f32(&self.shared.music) * master;
        self.music
            .set_mood(mood_of(self.shared.mood.load(Ordering::Relaxed)));
        let submerged_target = if self.shared.submerged.load(Ordering::Relaxed) {
            1.0
        } else {
            0.0
        };
        // A quarter of a second from dry to fully muffled, which is
        // about how long going under actually takes.
        let ramp = 4.0 / self.rate;

        let channels = channels.max(1);
        for frame in out.chunks_mut(channels) {
            let (mut left, mut right) = self.voice_frame();
            left *= sfx_gain;
            right *= sfx_gain;

            // Only the sound effects are muffled. Music is not coming
            // from the world, so putting it under water with everything
            // else would be a filter on the soundtrack of a film.
            self.submerged += (submerged_target - self.submerged).clamp(-ramp, ramp);
            if self.submerged > 0.0001 {
                self.low_left += self.alpha * (left - self.low_left);
                self.low_right += self.alpha * (right - self.low_right);
                left += (self.low_left - left) * self.submerged;
                right += (self.low_right - right) * self.submerged;
            }

            if music_gain > 0.0001 {
                let (music_left, music_right) = self.music.next();
                left += music_left * music_gain;
                right += music_right * music_gain;
            }

            let left = soft_clip(left);
            let right = soft_clip(right);

            match channels {
                1 => frame[0] = (left + right) * 0.5,
                _ => {
                    frame[0] = left;
                    frame[1] = right;
                    for extra in &mut frame[2..] {
                        *extra = 0.0;
                    }
                }
            }
        }
    }

    /// Sums every active voice and retires the ones that have finished.
    #[inline]
    fn voice_frame(&mut self) -> (f32, f32) {
        let mut left = 0.0;
        let mut right = 0.0;
        let mut i = 0;
        while i < self.voices.len() {
            match self.voices[i].next() {
                Some((l, r)) => {
                    left += l;
                    right += r;
                    i += 1;
                }
                None => {
                    // `swap_remove`: order does not matter and this does
                    // not shift the tail. No allocation either way.
                    self.voices.swap_remove(i);
                }
            }
        }
        (left, right)
    }

    /// Takes whatever the game thread has queued, if it can get the lock.
    fn drain_queue(&mut self) {
        let Ok(mut queue) = self.shared.queue.try_lock() else {
            return;
        };
        for start in queue.drain(..) {
            if self.voices.len() >= MAX_VOICES {
                // Steal the quietest, which is the one whose loss is
                // least likely to be noticed. If the new sound is
                // quieter than everything already playing, it is the one
                // that gets dropped.
                let (quietest, loudness) = self
                    .voices
                    .iter()
                    .enumerate()
                    .fold((0, f32::MAX), |(at, min), (i, v)| {
                        let l = v.loudness();
                        if l < min {
                            (i, l)
                        } else {
                            (at, min)
                        }
                    });
                if loudness >= start.gain {
                    continue;
                }
                self.voices.swap_remove(quietest);
            }
            // Equal-power panning, as in the music: the cheap square-root
            // law, which is inaudibly different from the cosine one and
            // does not need two trigonometric calls per sound.
            let pan = start.pan.clamp(-1.0, 1.0);
            let step = (start.clip.sample_rate as f64 / self.rate as f64)
                * start.pitch.clamp(0.25, 4.0) as f64;
            self.voices.push(Voice {
                clip: start.clip,
                at: 0.0,
                step,
                left: start.gain * (0.5 - pan * 0.5).sqrt(),
                right: start.gain * (0.5 + pan * 0.5).sqrt(),
            });
        }
    }
}

/// Keeps the sum of a lot of sounds inside the rails without the flat
/// top that hard clipping gives it.
///
/// A Padé approximant of `tanh`: two multiplies and a divide, monotone,
/// and exactly linear near zero -- so quiet material is untouched and
/// only an actual pile-up gets squeezed. Hard `clamp` was tried first
/// and a fire next to a gravel collapse buzzed.
#[inline]
fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_shared(rate: u32) -> Arc<Shared> {
        Arc::new(Shared::new(Arc::new(Bank::new(rate))))
    }

    /// A tenth of a second of noise to push through the mixer. Made here
    /// rather than picked from the bank: the bank is recordings now, and
    /// decoding four hundred of them to test panning is a slow way to get
    /// one clip.
    fn test_clip(rate: u32) -> Arc<Clip> {
        let mut rng = Rng::new(3);
        let samples = (0..rate as usize / 10).map(|_| (rng.bipolar() * 20_000.0) as i16).collect();
        Arc::new(Clip { samples, sample_rate: rate })
    }

    /// The mood crosses to the audio thread as one byte and comes back
    /// as an enum. A mood added to `mood_index` and forgotten in
    /// `mood_of` would not fail to build -- it would quietly play the
    /// menu's music in a thunderstorm.
    #[test]
    fn every_mood_survives_the_trip_to_the_audio_thread() {
        for mood in [
            Mood::Menu,
            Mood::Day,
            Mood::Night,
            Mood::Cave,
            Mood::Rain,
            Mood::Storm,
            Mood::Sea,
            Mood::Forest,
            Mood::Hearth,
            Mood::Peril,
        ] {
            assert_eq!(mood_of(mood_index(mood)), mood);
        }
    }

    #[test]
    fn soft_clip_is_transparent_when_it_can_be() {
        assert!((soft_clip(0.0) - 0.0).abs() < 1e-6);
        for x in [-0.3f32, -0.1, 0.05, 0.2] {
            assert!((soft_clip(x) - x).abs() < 0.01, "{x} was bent");
        }
        // ...and never lets anything out of range.
        for x in [-40.0f32, -3.0, 3.0, 40.0] {
            assert!(soft_clip(x).abs() <= 1.0, "{x} escaped");
        }
    }

    #[test]
    fn a_queued_sound_is_heard_and_then_stops() {
        let rate = 22_050;
        let shared = test_shared(rate);
        shared.set_volumes(1.0, 1.0, 0.0);
        let clip = test_clip(rate);
        let frames = clip.len();
        shared.push(Start { clip, gain: 1.0, pan: 0.0, pitch: 1.0 });

        let mut mixer = Mixer::new(shared, rate as f32, 1);
        let mut out = vec![0.0f32; frames * 2];
        mixer.fill(&mut out, 2);
        assert!(out.iter().any(|s| s.abs() > 0.01), "nothing was heard");

        // The voice has run out; the next buffer is silent.
        let mut after = vec![0.0f32; 256];
        mixer.fill(&mut after, 2);
        assert!(after.iter().all(|s| s.abs() < 1e-6), "the voice never ended");
    }

    /// Panning hard left must leave the right channel empty, or
    /// positional audio is decorative.
    #[test]
    fn panning_reaches_the_ends() {
        let rate = 22_050;
        let shared = test_shared(rate);
        shared.set_volumes(1.0, 1.0, 0.0);
        let clip = test_clip(rate);
        shared.push(Start { clip: clip.clone(), gain: 1.0, pan: -1.0, pitch: 1.0 });

        let mut mixer = Mixer::new(shared, rate as f32, 1);
        let mut out = vec![0.0f32; clip.len() * 2];
        mixer.fill(&mut out, 2);
        let left: f32 = out.iter().step_by(2).map(|s| s.abs()).sum();
        let right: f32 = out.iter().skip(1).step_by(2).map(|s| s.abs()).sum();
        assert!(left > 0.01, "nothing on the left");
        assert!(right < 1e-6, "hard left leaked {right} into the right");
    }

    /// More sounds than there are voices must not grow the voice list,
    /// allocate, or fail to play the loud ones.
    #[test]
    fn the_voice_cap_holds() {
        let rate = 22_050;
        let shared = test_shared(rate);
        shared.set_volumes(1.0, 1.0, 0.0);
        for i in 0..MAX_QUEUED * 2 {
            let clip = test_clip(rate);
            shared.push(Start {
                clip,
                gain: 0.1 + (i % 8) as f32 * 0.1,
                pan: 0.0,
                pitch: 1.0,
            });
        }
        let mut mixer = Mixer::new(shared.clone(), rate as f32, 1);
        let mut out = vec![0.0f32; 512];
        mixer.fill(&mut out, 2);
        assert!(mixer.voices.len() <= MAX_VOICES);
        assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        assert!(shared.dropped.load(Ordering::Relaxed) > 0, "the cap never bit");
    }

    /// A mono device gets the sum rather than the left channel, and a
    /// four-channel one gets silence in the two it is not using rather
    /// than whatever was in the buffer.
    #[test]
    fn odd_channel_counts_are_filled_sensibly() {
        let rate = 22_050;
        for channels in [1usize, 2, 4] {
            let shared = test_shared(rate);
            shared.set_volumes(1.0, 1.0, 0.0);
            let clip = test_clip(rate);
            shared.push(Start { clip, gain: 1.0, pan: 1.0, pitch: 1.0 });
            let mut mixer = Mixer::new(shared, rate as f32, 1);
            let mut out = vec![9.0f32; channels * 300];
            mixer.fill(&mut out, channels);
            assert!(out.iter().all(|s| s.abs() <= 1.0), "{channels} channels leaked");
            if channels > 2 {
                assert!(
                    out.chunks(channels).all(|f| f[2..].iter().all(|s| *s == 0.0)),
                    "the spare channels were left dirty"
                );
            }
        }
    }
}
