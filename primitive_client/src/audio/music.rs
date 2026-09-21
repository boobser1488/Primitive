//! The composer: music that is written while it is played.
//!
//! ## Why not a soundtrack
//!
//! A game this size cannot ship an hour of recorded music, and half an
//! hour of it on a loop is worse than none -- the loop point is the only
//! thing anybody remembers. What a world like this actually wants is
//! *weather*: something that is there when you look up and gone when you
//! stop listening, that never repeats exactly, and that knows whether
//! you are underground.
//!
//! So there is no track. There is a set of rules, a table of
//! hand-written motifs, and a random number generator, and the music is
//! decided one note ahead of the speaker.
//!
//! ## What is written by hand and what is not
//!
//! * **By hand:** the scales, the chord progressions, the motifs in
//!   [`MOTIFS`], and the personality of each [`Mood`] -- including
//!   which *kinds* of phrase it is allowed to play, which is what makes
//!   a cave and a meadow differ in more than how far apart their notes
//!   are (see [`Colour`]). Purely generated melody is famously aimless
//!   -- it wanders because nothing in it was ever *going* anywhere --
//!   and the motifs are the thing that gives it somewhere to go.
//! * **Generated:** which motif, where in it a quotation starts, at
//!   which octave, over which chord, how loud each note of the phrase
//!   is, how long the piece runs, and how long the silence after it
//!   lasts. That is enough variation that two sessions never hear the
//!   same piece, and little enough that every piece sounds like it came
//!   from the same place.
//!
//! ## There is no beat, and that is deliberate
//!
//! The first version of this had a metre: eighth notes on a grid, a bass
//! note on every downbeat, a four-bar chord progression, and a melody
//! over the top. Every one of those is a good idea and together they are
//! **a tune** -- and a tune playing while somebody chops wood is a video
//! game announcing itself, however carefully the notes were chosen. The
//! timbre was fixed first (see [`Timbre`]) and it was not enough,
//! because what reads as arcade is not the sound of the notes, it is
//! that there are bars.
//!
//! So there is no tempo in this file any more. What is left is two
//! layers on unrelated clocks:
//!
//! * a **drone** -- two or three sustained tones, each ten to thirty
//!   seconds long, entering and leaving independently, so the chord is
//!   never struck and never quite the same;
//! * a **voice** -- one note, occasionally a slow fragment of one of
//!   the motifs, every several seconds, at no fixed distance from
//!   anything else.
//!
//! Nothing lands on a beat because there is no beat to land on. The
//! harmony still moves -- the modal centre drifts through the
//! progression every twenty or forty seconds -- but it drifts rather
//! than changing on a count.
//!
//! ## The silence is part of it
//!
//! A piece runs for twenty or thirty bars and then stops for anywhere up
//! to a minute and a half. Continuous music in a game whose sounds *are*
//! the feedback -- footsteps telling you what you are standing on, a
//! pick telling you what you are in -- competes with the thing the
//! player is listening to. The rests are what make the music an event
//! rather than a floor.
//!
//! ## It runs on the audio thread
//!
//! [`Composer::next`] produces one stereo frame and is called for every
//! sample, inside the device callback. That is the reason for every
//! shape in this file: a fixed voice array rather than a `Vec`, an
//! eight-byte random generator rather than a shared one, no allocation,
//! no locks, no `Instant::now`. The note scheduling happens on the
//! sample where a note falls due, so a bar of music costs the same as a
//! bar of silence plus six oscillators.

use super::clip::Rng;

/// What the music should be about.
///
/// The client decides this from where the player is and what is
/// happening to them -- see `soundscape` -- and hands it over as one
/// value. Deliberately small: a mood per situation would mean writing a
/// piece per situation, which is a soundtrack again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mood {
    /// The menus. Warm and slow: it is the first thing anybody hears.
    #[default]
    Menu,
    /// Above ground, in daylight.
    Day,
    /// Above ground, after dark. The same world, a fifth of the tempo
    /// taken out and the third flattened.
    Night,
    /// Below the surface. Barely music at all -- long notes, wide
    /// spaces, and the most reverb of any mood.
    Cave,
    /// Rain. Dorian, which is the mode that manages to be minor without
    /// being sad.
    Rain,
    /// A storm proper: lightning, and rain the weather system agreed to
    /// call a storm.
    ///
    /// **Split off from [`Mood::Rain`] because they were the same
    /// music.** Standing in drizzle and standing in a thunderstorm were
    /// one recipe, which made the mood system claim a distinction the
    /// ear could not find. A storm is the flat second -- Phrygian, the
    /// one mode that sounds like weather rather than like a key -- low
    /// and with the widest reverb here after the cave.
    Storm,
    /// Open water. Wide, slow, and the only mood built on Mixolydian:
    /// the flat seventh never quite closes, which is what being a long
    /// way from a shore feels like.
    Sea,
    /// Under a canopy, in daylight. Major pentatonic and a light bass --
    /// the one mood with no semitone anywhere in its scale, so nothing
    /// it plays can be sour.
    Forest,
    /// A fire, with a roof over it. The warmest and the closest: the
    /// least reverb of any mood, because a hearth is a small room and
    /// every other mood is outdoors or underground.
    Hearth,
    /// Badly hurt. Faster, lower, and the only mood with any urgency in
    /// it.
    Peril,
}

impl Mood {
    /// The numbers and the phrase palette that make one mood different
    /// from another.
    fn recipe(self) -> Recipe {
        match self {
            Mood::Menu => Recipe {
                root: 220.00, // A3
                scale: MAJOR,
                progression: &[0, 3, 4, 3],
                note_gap: (5.0, 11.0),
                drone_gap: (9.0, 18.0),
                centre_gap: (20.0, 40.0),
                colours: &[Colour::Settled, Colour::Falling, Colour::Open],
                melody: 0.55,
                bass: 0.5,
                pad: 1.0,
                reverb: 0.30,
            },
            Mood::Day => Recipe {
                root: 261.63, // C4
                scale: LYDIAN,
                progression: &[0, 4, 5, 3],
                note_gap: (4.0, 9.0),
                drone_gap: (8.0, 16.0),
                centre_gap: (18.0, 34.0),
                colours: &[Colour::Settled, Colour::Rising, Colour::Open],
                melody: 0.75,
                bass: 0.6,
                pad: 0.8,
                reverb: 0.26,
            },
            Mood::Night => Recipe {
                root: 220.00,
                scale: MINOR,
                progression: &[0, 5, 3, 4],
                note_gap: (6.0, 14.0),
                drone_gap: (11.0, 22.0),
                centre_gap: (26.0, 48.0),
                colours: &[Colour::Falling, Colour::Sparse, Colour::Open],
                melody: 0.5,
                bass: 0.55,
                pad: 1.0,
                reverb: 0.36,
            },
            Mood::Cave => Recipe {
                root: 146.83, // D3
                scale: MINOR_PENTATONIC,
                progression: &[0, 0, 3, 3],
                note_gap: (9.0, 22.0),
                drone_gap: (14.0, 30.0),
                centre_gap: (40.0, 75.0),
                // **Two colours where every other mood has three.** A
                // cave has less to say, and saying it with a narrower
                // set of phrases is how that is heard rather than
                // announced.
                colours: &[Colour::Sparse, Colour::Open],
                melody: 0.3,
                bass: 0.7,
                pad: 1.0,
                reverb: 0.5,
            },
            Mood::Rain => Recipe {
                root: 196.00, // G3
                scale: DORIAN,
                progression: &[0, 3, 4, 0],
                note_gap: (5.0, 12.0),
                drone_gap: (10.0, 20.0),
                centre_gap: (20.0, 38.0),
                colours: &[Colour::Settled, Colour::Falling, Colour::Restless],
                melody: 0.6,
                bass: 0.6,
                pad: 0.9,
                reverb: 0.34,
            },
            Mood::Storm => Recipe {
                root: 155.56, // E flat 3
                scale: PHRYGIAN,
                // The flat second used as a chord centre, which is the
                // sound nothing else here makes.
                progression: &[0, 1, 4, 1],
                note_gap: (4.0, 9.0),
                drone_gap: (7.0, 15.0),
                centre_gap: (14.0, 26.0),
                colours: &[Colour::Restless, Colour::Falling, Colour::Open],
                melody: 0.55,
                bass: 0.85,
                pad: 0.9,
                reverb: 0.42,
            },
            Mood::Sea => Recipe {
                root: 130.81, // C3
                scale: MIXOLYDIAN,
                progression: &[0, 4, 6, 4],
                // The widest gaps of any mood above ground. A horizon is
                // not a busy place.
                note_gap: (7.0, 16.0),
                drone_gap: (12.0, 26.0),
                centre_gap: (30.0, 60.0),
                colours: &[Colour::Rising, Colour::Sparse, Colour::Open],
                melody: 0.45,
                bass: 0.75,
                pad: 1.0,
                reverb: 0.42,
            },
            Mood::Forest => Recipe {
                root: 233.08, // B flat 3
                scale: MAJOR_PENTATONIC,
                progression: &[0, 2, 4, 2],
                note_gap: (5.0, 12.0),
                drone_gap: (9.0, 19.0),
                centre_gap: (22.0, 42.0),
                colours: &[Colour::Settled, Colour::Rising, Colour::Sparse],
                melody: 0.65,
                bass: 0.5,
                pad: 0.85,
                reverb: 0.30,
            },
            Mood::Hearth => Recipe {
                root: 174.61, // F3
                scale: MAJOR,
                progression: &[0, 4, 3, 0],
                note_gap: (6.0, 13.0),
                drone_gap: (10.0, 20.0),
                centre_gap: (24.0, 44.0),
                colours: &[Colour::Settled, Colour::Falling, Colour::Sparse],
                melody: 0.5,
                bass: 0.65,
                pad: 1.0,
                // A small room. Everything else here is a valley, a
                // cave or the sea.
                reverb: 0.18,
            },
            Mood::Peril => Recipe {
                root: 164.81, // E3
                scale: MINOR,
                progression: &[0, 1, 0, 4],
                note_gap: (3.0, 7.0),
                drone_gap: (6.0, 13.0),
                // The harmony moves twice as often as anywhere else,
                // which is the only thing in this file that hurries.
                centre_gap: (10.0, 20.0),
                colours: &[Colour::Restless, Colour::Rising, Colour::Falling],
                melody: 0.7,
                bass: 0.9,
                pad: 0.6,
                reverb: 0.22,
            },
        }
    }

    /// Every mood there is. Written out rather than derived, because the
    /// tests below iterate it and a mood missing from this list is a
    /// mood nothing checks.
    #[cfg(test)]
    const ALL: &'static [Mood] = &[
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
    ];
}

struct Recipe {
    /// Hz of the tonic, in the octave the bass plays in.
    root: f32,
    /// Semitones above the tonic, one octave's worth.
    scale: &'static [i32],
    /// Which scale degree each bar's chord is rooted on. Four bars,
    /// repeated -- the oldest trick there is, and the reason a piece
    /// made of random motifs still sounds like it has a shape.
    progression: &'static [i32],
    /// Seconds between one sung note and the next, at the two ends of
    /// the range it is drawn from. There is no tempo -- see the module
    /// note -- so this is the only thing that decides how busy a mood
    /// is.
    note_gap: (f32, f32),
    /// ...and between one drone tone being renewed and the next.
    drone_gap: (f32, f32),
    /// ...and between one chord of the progression and the next.
    ///
    /// **Per mood rather than the one 20-to-40-second range every mood
    /// used to share.** How often the harmony moves is most of how
    /// urgent a piece feels, and having it fixed meant a cave and a
    /// player bleeding out changed chord at the same rate -- so two
    /// moods that were supposed to be opposites agreed about the one
    /// thing a listener actually follows.
    centre_gap: (f32, f32),
    /// Which kinds of phrase this mood draws on. See [`Colour`].
    colours: &'static [Colour],
    /// How likely a note event is to be a short fragment of a motif
    /// rather than a single tone.
    melody: f32,
    bass: f32,
    pad: f32,
    /// How much of the delay network is mixed back in.
    reverb: f32,
}

const MAJOR: &[i32] = &[0, 2, 4, 5, 7, 9, 11];
const MINOR: &[i32] = &[0, 2, 3, 5, 7, 8, 10];
const DORIAN: &[i32] = &[0, 2, 3, 5, 7, 9, 10];
const LYDIAN: &[i32] = &[0, 2, 4, 6, 7, 9, 11];
/// The flat second is the whole of it: the one mode in this file that
/// does not sound like a key somebody chose.
const PHRYGIAN: &[i32] = &[0, 1, 3, 5, 7, 8, 10];
/// Major with a flat seventh, so the octave never closes.
const MIXOLYDIAN: &[i32] = &[0, 2, 4, 5, 7, 9, 10];
/// Named for which one it is, because there are two now and the bare
/// word was ambiguous the moment the second arrived.
const MINOR_PENTATONIC: &[i32] = &[0, 3, 5, 7, 10];
/// No semitone anywhere in it, which is why nothing written in it can
/// come out sour however the chord under it moves.
const MAJOR_PENTATONIC: &[i32] = &[0, 2, 4, 7, 9];

/// A rest, as a scale degree. Any degree far enough outside the useful
/// range to be unmistakable.
const REST: i32 = i32::MIN;

/// What a phrase is *for*, which is how a mood picks one.
///
/// **The palette, not just the list.** Every mood used to draw from the
/// same six motifs with the same odds, so the only thing that made a
/// cave different from a meadow was how far apart its notes were. Two
/// moods that share every phrase they can play share most of their
/// character, whatever their scale says. A colour is one word about
/// what a phrase does; a mood names the two or three words it is
/// willing to say, and the rest of the table is closed to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Colour {
    /// Goes somewhere and comes home. The backbone: a piece made only
    /// of the other five never appears to arrive anywhere.
    Settled,
    /// Climbs, and stops while it is still up there.
    Rising,
    /// Comes down, sometimes past the root. The answer to a `Rising`,
    /// which is why no mood has one without the other or without a
    /// `Settled` to land on.
    Falling,
    /// Ends on a degree that is not in the chord -- the second, the
    /// sixth, the seventh -- so the phrase is a question. **The thing
    /// the old table had none of:** all six of its motifs closed on a
    /// chord tone, which is why a piece made of them sounded like a
    /// series of small conclusions.
    Open,
    /// Mostly silence: rests inside the phrase, or one long note and
    /// nothing else.
    Sparse,
    /// Short values, small steps, no landing. The only colour with any
    /// hurry in it.
    Restless,
}

/// One hand-written phrase: the degrees, their relative lengths, and
/// what it is for.
struct Motif {
    /// Degrees relative to the chord root, and how long each is held
    /// relative to the others. `sing` turns a length into seconds.
    notes: &'static [(i32, u32)],
    colour: Colour,
}

/// The shortest and longest a motif may be, in its own units.
///
/// **A range where there used to be a single number.** Every motif was
/// required to total exactly eight, a rule left over from when there
/// were bars for them to fill. There are no bars any more -- nothing in
/// the scheduler reads a motif's total at all -- so all the rule still
/// did was guarantee that every phrase in the game was the same size of
/// gesture, which is one of the ways a long session gave itself away.
/// What is worth keeping is the bound: a two-unit phrase is not a
/// phrase, and a twenty-unit one is a tune.
/// Nothing but the tests reads either -- they are a rule about the
/// table below rather than something the scheduler consults.
#[cfg(test)]
const MOTIF_MIN_UNITS: u32 = 4;
#[cfg(test)]
const MOTIF_MAX_UNITS: u32 = 12;

/// The phrases every melody is built from.
///
/// Degrees are relative to the chord's root, so the same motif is a
/// different set of notes over each chord -- which is most of why a
/// handful of short phrases do not wear out.
///
/// They are short on purpose. A long generated-sounding phrase is worse
/// than a short human-sounding one repeated, and these are meant to be
/// recognised rather than followed.
///
/// **What was wrong with the six this grew out of**, all of it audible
/// over an evening rather than over a minute:
///
/// * every one of them ended on a chord tone, and three of the six
///   ended on the same degree by the same descending step;
/// * every one of them was exactly eight units long;
/// * one of the six contained a rest and the other five were solid;
/// * and `sing` took its single notes from the *head* of a motif, so
///   over half the individual notes in a piece were a root or a fifth
///   and the tails of the longer phrases were almost never heard.
///
/// The first three are fixed here and the fourth in `sing`.
const MOTIFS: &[Motif] = &[
    // ---------------------------------------------------------- settled
    Motif {
        // Rising and settling back. The plainest one there is, and the
        // one that fits over any chord in any mood that can play it.
        notes: &[(0, 2), (2, 2), (4, 2), (2, 2)],
        colour: Colour::Settled,
    },
    Motif {
        // A step away from home and a step back, all long notes. Three
        // notes is the smallest thing that is still a sentence, and a
        // mood that is mostly silence needs one.
        notes: &[(0, 3), (1, 2), (0, 3)],
        colour: Colour::Settled,
    },
    Motif {
        // Two quick notes and a long one. The shortest motif here: five
        // units, so a phrase built on it is over before the others
        // would have reached their third note.
        notes: &[(2, 1), (4, 1), (0, 3)],
        colour: Colour::Settled,
    },
    Motif {
        // A turn around the fifth. Ends on the fifth rather than the
        // root -- stable, but not finished, which is what lets it be
        // followed by anything.
        notes: &[(4, 2), (5, 1), (4, 1), (2, 2), (4, 2)],
        colour: Colour::Settled,
    },
    // ----------------------------------------------------------- rising
    Motif {
        // A run up to a held note, then a step down off it.
        notes: &[(0, 1), (1, 1), (2, 1), (4, 3), (2, 2)],
        colour: Colour::Rising,
    },
    Motif {
        // Climbing in thirds and out of breath at the top: the last
        // note is the seventh, held longest, and nothing resolves it.
        notes: &[(0, 2), (2, 2), (4, 2), (6, 3)],
        colour: Colour::Rising,
    },
    Motif {
        // A leap and a sigh. The only phrase that moves a fifth in one
        // step, which is why there is exactly one of it.
        notes: &[(0, 1), (5, 4), (4, 2)],
        colour: Colour::Rising,
    },
    // ---------------------------------------------------------- falling
    Motif {
        // A fall from the fifth onto a long tonic.
        notes: &[(4, 1), (3, 1), (2, 2), (0, 4)],
        colour: Colour::Falling,
    },
    Motif {
        // Straight down past the root and left there. This one used to
        // climb back to the tonic at the end, which made it the third
        // phrase of six to finish at home by a step.
        notes: &[(4, 2), (2, 1), (0, 1), (-2, 3)],
        colour: Colour::Falling,
    },
    Motif {
        // Three long notes letting go of one another. Eleven units, the
        // longest thing here, and the only motif with no note shorter
        // than three.
        notes: &[(6, 4), (4, 3), (2, 4)],
        colour: Colour::Falling,
    },
    Motif {
        // Stumbling down, a pause, and then the bottom. The rest is
        // inside the descent rather than at either end of it, which is
        // where a person breathes.
        notes: &[(4, 1), (3, 1), (2, 1), (REST, 2), (0, 3)],
        colour: Colour::Falling,
    },
    // ------------------------------------------------------------- open
    Motif {
        // Ends on the second and stays there: the plainest question in
        // the table.
        notes: &[(0, 2), (2, 2), (1, 4)],
        colour: Colour::Open,
    },
    Motif {
        // Up to the sixth and stopped. Six units, so it is a question
        // asked quickly.
        notes: &[(2, 1), (4, 2), (5, 3)],
        colour: Colour::Open,
    },
    Motif {
        // Two notes a seventh apart, both held. Almost nothing -- and
        // an interval left hanging is more of an event than a run of
        // notes that resolves.
        notes: &[(0, 4), (6, 4)],
        colour: Colour::Open,
    },
    // ----------------------------------------------------------- sparse
    Motif {
        // With a hole in it. The rest is the point -- a piece of
        // nothing but solid phrases is a machine.
        notes: &[(2, 2), (REST, 1), (0, 2), (-3, 3)],
        colour: Colour::Sparse,
    },
    Motif {
        // The same note twice with silence between, which is what a
        // bell does and what no scale-shaped phrase can imitate. On the
        // third rather than the root, because the sparse phrases are
        // the whole of what a cave plays and three of the four would
        // otherwise sit on the same two degrees -- which is how a mood
        // with a small palette ends up sounding like one note.
        notes: &[(2, 3), (REST, 2), (2, 4)],
        colour: Colour::Sparse,
    },
    Motif {
        // Starts with the rest. Every other phrase begins by making a
        // sound, and a mood that is mostly waiting needs one that
        // begins by not.
        notes: &[(REST, 2), (-2, 3), (0, 4)],
        colour: Colour::Sparse,
    },
    Motif {
        // One long note and nothing else. It is a motif because the
        // scheduler has to be able to *choose* it: a single held fifth
        // is the right thing to say in a cave, and taking it from the
        // head of a longer phrase would mean saying more.
        notes: &[(4, 6)],
        colour: Colour::Sparse,
    },
    // --------------------------------------------------------- restless
    Motif {
        // Three quick steps and a landing.
        notes: &[(0, 1), (1, 1), (2, 1), (1, 1), (0, 2)],
        colour: Colour::Restless,
    },
    Motif {
        // Turning on itself and refusing to settle: six notes, five of
        // them the shortest value there is.
        notes: &[(2, 1), (1, 1), (2, 1), (3, 1), (2, 1), (4, 2)],
        colour: Colour::Restless,
    },
    Motif {
        // Down in a hurry, and the one place a phrase is allowed to
        // arrive home abruptly.
        notes: &[(6, 1), (4, 1), (3, 2), (2, 1), (0, 3)],
        colour: Colour::Restless,
    },
];

/// What an oscillator is pretending to be.
///
/// **None of the three is a raw waveform any more, and that is the whole
/// of the fix.** The lead used to be a triangle wave, which is one of
/// the five voices an NES had and is therefore the single most
/// recognisable "this is a video game from 1985" sound there is. It does
/// not matter how carefully the notes are chosen underneath a timbre
/// like that; the timbre is what the ear names.
///
/// What replaced it is a sine with a little second and third harmonic
/// and a breath of noise at the front of each note -- roughly a wooden
/// flute, which is an instrument a person in this world could plausibly
/// have made. That is not a coincidence: the game is a stone age, and
/// the music should be playable by somebody living in one.
#[derive(Clone, Copy, PartialEq)]
enum Timbre {
    /// Two sines a few cents apart. Slow in, slow out; the bed the rest
    /// sits on.
    Pad,
    /// The tune. A sine fundamental with the first two harmonics under
    /// it, a slow attack, and a slight vibrato that arrives *after* the
    /// note does -- which is what a player's finger does and what a
    /// synthesiser never does on its own.
    Lead,
    /// A sine with a little of its second harmonic, low down.
    Bass,
}

/// One note being played.
///
/// A plain struct in a fixed array, all of them always present, most of
/// them idle. Allocating a voice on a note-on would mean allocating on
/// the audio thread, which is the one thing that is genuinely forbidden
/// there.
#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    timbre: Timbre,
    freq: f32,
    phase: f32,
    phase_b: f32,
    phase_c: f32,
    gain: f32,
    /// Seconds since the note started, and how long it lasts in total.
    age: f32,
    life: f32,
    attack: f32,
    pan: f32,
    /// A ramp on top of the envelope, and how fast it falls, for a note
    /// that has to stop before it was going to.
    ///
    /// **The end of a piece used to be a cut.** `silence` set `active`
    /// to false on every voice, which -- with two or three drone tones
    /// halfway through thirty-second envelopes -- took the music from
    /// whatever it was doing to nothing between one sample and the
    /// next. That is a click, and it arrived at the one moment the
    /// music was supposed to be disappearing.
    ///
    /// Measured over four minutes of each mood, as how loud the last
    /// ten milliseconds before a silence were against the loudest ten
    /// milliseconds of the piece: `Peril` 27%, `Night` 21%, `Cave` 11%
    /// -- and 3% or less everywhere once the ramp existed. See
    /// `level_before_silence` in the tests for why that is the measure
    /// and not the more obvious two.
    ///
    /// Shortening `life` instead was tried and is worse: the envelope
    /// is a curve from the attack to `life`, so moving `life` moves the
    /// *current* value of it, which is the same click with arithmetic
    /// in front of it.
    release: f32,
    release_rate: f32,
}

impl Voice {
    const IDLE: Voice = Voice {
        active: false,
        timbre: Timbre::Pad,
        freq: 0.0,
        phase: 0.0,
        phase_b: 0.0,
        phase_c: 0.0,
        gain: 0.0,
        age: 0.0,
        life: 0.0,
        attack: 0.0,
        pan: 0.0,
        release: 1.0,
        release_rate: 0.0,
    };

    /// One sample, before panning.
    ///
    /// `noise` is a sample of white noise the caller has already drawn.
    /// Passed in rather than generated here because the generator lives
    /// on the composer and a voice does not get to borrow it mid-loop --
    /// and because every voice wanting one on every sample would be
    /// three times the calls for no audible difference.
    #[inline]
    fn next(&mut self, dt: f32, noise: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        self.age += dt;
        if self.age >= self.life {
            self.active = false;
            return 0.0;
        }
        // A note that was asked to stop. The ramp is multiplied into
        // the envelope below rather than replacing it, so a voice that
        // was already dying away is not made louder by being told to
        // die away faster.
        if self.release_rate > 0.0 {
            self.release -= self.release_rate * dt;
            if self.release <= 0.0 {
                self.active = false;
                return 0.0;
            }
        }

        // Attack, then a curve down to nothing at `life`. The envelope
        // reaching exactly zero at the end is what lets a voice be
        // switched off without a click.
        let env = if self.age < self.attack {
            self.age / self.attack
        } else {
            let t = (self.age - self.attack) / (self.life - self.attack).max(1e-4);
            let fall = 1.0 - t;
            match self.timbre {
                // A pad holds and then goes; a blown note holds too and
                // then stops; only the bass is struck.
                Timbre::Pad => fall.powf(1.2),
                Timbre::Lead => fall.powf(1.1),
                Timbre::Bass => fall.powf(2.2),
            }
        };

        let out = match self.timbre {
            Timbre::Pad => {
                self.phase = (self.phase + self.freq * dt).fract();
                // Detuned by about six cents, which is slow enough to
                // hear as movement rather than as being out of tune.
                self.phase_b = (self.phase_b + self.freq * 1.0035 * dt).fract();
                (sine(self.phase) + sine(self.phase_b)) * 0.5
            }
            Timbre::Lead => {
                // Vibrato that fades in over the first fifth of a
                // second. A note that wobbles from its first instant is
                // a machine; one that steadies and then wavers is
                // somebody holding it.
                let depth = (self.age / 0.2).min(1.0) * 0.004;
                let wobble = 1.0 + depth * sine(self.age * 5.2);
                self.phase = (self.phase + self.freq * wobble * dt).fract();
                self.phase_b = (self.phase_b + self.freq * 2.0 * dt).fract();
                self.phase_c = (self.phase_c + self.freq * 3.0 * dt).fract();
                // Fundamental, an octave, a twelfth: a hollow, woody
                // spectrum rather than the bright odd-harmonic ladder a
                // triangle gives.
                //
                // **And the harmonics fade out as the note goes up**,
                // which is what every wind instrument does and what a
                // synthesiser never does on its own. A fixed recipe
                // that is warm at two hundred hertz is a whistle at
                // eight hundred, because its third harmonic has climbed
                // into the part of the spectrum the ear is sharpest in.
                let brightness = (600.0 / self.freq.max(120.0)).clamp(0.25, 1.0);
                let tone = sine(self.phase)
                    + sine(self.phase_b) * 0.22 * brightness
                    + sine(self.phase_c) * 0.09 * brightness * brightness;
                // Breath, only at the very front of the note, where a
                // real wind instrument puts it.
                let air = noise * 0.25 * (1.0 - (self.age / 0.06).min(1.0));
                tone * 0.8 + air
            }
            Timbre::Bass => {
                self.phase = (self.phase + self.freq * dt).fract();
                self.phase_b = (self.phase_b + self.freq * 2.0 * dt).fract();
                sine(self.phase) + sine(self.phase_b) * 0.18
            }
        };
        out * env * self.gain * self.release
    }

    /// Asks this note to die away over `seconds` from wherever it is.
    #[inline]
    fn let_go(&mut self, seconds: f32) {
        // Whichever ramp is already steeper wins, so a second request
        // cannot make a voice take longer to stop.
        self.release_rate = self.release_rate.max(1.0 / seconds.max(0.01));
    }
}

/// How long a note takes to die away when a piece ends under it.
///
/// Long enough that the fall is inaudible as an event and short enough
/// that the rest afterwards is actually a rest -- the shortest silence
/// the composer ever schedules is twenty-five seconds.
const RELEASE: f32 = 0.9;

#[inline]
fn sine(phase: f32) -> f32 {
    (phase * std::f32::consts::TAU).sin()
}

/// The window the sung line is allowed to live in, in Hz.
///
/// Roughly G3 to B flat 5, which is a singer's range and not by
/// accident: [`Timbre::Lead`] is a wooden flute and a person made it.
const LEAD_FLOOR: f32 = 165.0;
const LEAD_CEILING: f32 = 1250.0;

/// Drops or lifts a note by whole octaves until it is in that window.
///
/// **Nothing used to stop a note climbing.** A degree is the chord's
/// root plus the motif's own offset plus the phrase's octave, and the
/// three of them add up: measured over twenty minutes, `Day` sang as
/// high as 2637 Hz (E7) and every mood reached at least 1480. That is
/// above where the melody was ever meant to be, it is the band the
/// squeal test exists to keep the music out of, and it puts the third
/// harmonic of a *background* line at eight kilohertz.
///
/// By octaves rather than by clamping, because a clamp turns every note
/// that runs off the top into the same note -- a melody stuck on its
/// own ceiling, which is a worse fault than the one being fixed. An
/// octave keeps the note in the scale and keeps the shape of the phrase
/// recognisable, which is exactly what an instrument with a limited
/// compass makes a player do.
fn fold_into_register(mut freq: f32) -> f32 {
    // A zero or a NaN would make one of the loops below run for ever,
    // on the audio thread, which is the worst place in the program to
    // find that out.
    if !freq.is_normal() || freq <= 0.0 {
        return LEAD_FLOOR;
    }
    while freq > LEAD_CEILING {
        freq *= 0.5;
    }
    while freq < LEAD_FLOOR {
        freq *= 2.0;
    }
    freq
}

/// A ping-pong delay, which is as much reverb as this needs.
///
/// Two lines of different lengths feeding each other's input is a
/// two-tap echo that spreads across the stereo field and decays into a
/// wash. A real reverb (a bank of combs into a chain of allpasses)
/// sounded better on a held pad and cost eight times as much on a thread
/// that has a hard deadline; this is a tenth of a millisecond and does
/// the one job that matters, which is stopping the notes sounding like
/// they are happening inside the speaker.
struct Delay {
    left: Vec<f32>,
    right: Vec<f32>,
    at_left: usize,
    at_right: usize,
    /// One pole of low-pass in each feedback path, and its coefficient.
    ///
    /// **A reflection loses its highs and this one did not.** Every
    /// surface and the air itself absorb treble far faster than bass,
    /// which is why a real room's tail gets darker as it dies away. A
    /// feedback loop with no damping keeps the top end at full strength
    /// for every pass and stacks it up -- the metallic ring that makes
    /// a cheap reverb identifiable, and half of what was piercing here.
    damp_left: f32,
    damp_right: f32,
    damping: f32,
}

impl Delay {
    fn new(rate: f32) -> Delay {
        // Prime-ish lengths, so the two taps do not line up into a
        // single flutter.
        let a = (rate * 0.0937) as usize + 1;
        let b = (rate * 0.1311) as usize + 1;
        // Two and a half kilohertz: the tail keeps its body and loses
        // its edge, which is what a room sounds like.
        let dt = 1.0 / rate;
        let rc = 1.0 / (std::f32::consts::TAU * 2_500.0);
        Delay {
            left: vec![0.0; a],
            right: vec![0.0; b],
            at_left: 0,
            at_right: 0,
            damp_left: 0.0,
            damp_right: 0.0,
            damping: dt / (rc + dt),
        }
    }

    #[inline]
    fn run(&mut self, input: f32, amount: f32) -> (f32, f32) {
        let tail_left = self.left[self.at_left];
        let tail_right = self.right[self.at_right];
        // Darkened on the way round, once per pass -- see `damping`.
        self.damp_left += self.damping * (tail_right - self.damp_left);
        self.damp_right += self.damping * (tail_left - self.damp_right);
        // Each line is fed by the other, which is what makes it bounce.
        self.left[self.at_left] = input + self.damp_left * 0.42;
        self.right[self.at_right] = input + self.damp_right * 0.42;
        self.at_left = (self.at_left + 1) % self.left.len();
        self.at_right = (self.at_right + 1) % self.right.len();
        (
            input + tail_left * amount,
            input + tail_right * amount,
        )
    }
}

/// What the composer is doing right now.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// Between pieces. The usual state, by design.
    Resting,
    Playing,
    /// The mood changed under a piece that was already running. Rather
    /// than cut, it fades and then rests briefly -- a hard switch on
    /// walking into a cave is a jump-scare.
    Fading,
}

/// The whole music system. One of these lives inside the mixer.
pub struct Composer {
    rate: f32,
    rng: Rng,
    voices: [Voice; Composer::VOICES],
    delay: Delay,

    /// What the game has asked for, and what is actually sounding.
    wanted: Mood,
    playing: Mood,

    phase: Phase,
    /// Samples until the next sung note, and until the drone is
    /// renewed. **Two clocks, unrelated on purpose**: anything that
    /// shares a clock with anything else is a beat, and a beat is what
    /// this file was rewritten to get rid of.
    to_next_note: f32,
    to_next_drone: f32,
    /// Samples until the modal centre drifts, and where it is now.
    to_next_centre: f32,
    centre: usize,
    /// Samples of piece left, and of silence when resting.
    piece_left: f32,
    rest_left: f32,
    /// The phrase being sung, if one is under way. See `sing`.
    phrase_motif: usize,
    phrase_at: usize,
    phrase_octave: i32,
    phrase_left: u32,
    /// The last note sung, and how many times in a row it has been the
    /// same one. See the guard in `sing`.
    last_freq: f32,
    same_note: u32,
    /// How many notes the phrase was going to be, which is the only way
    /// to know how far through it a note is -- and therefore how loud
    /// it should be. See the contour in `sing`.
    phrase_length: u32,
    /// How many times a note had to take a voice away from another one.
    ///
    /// **Kept in the build the player runs, not just in tests.** The
    /// claim above [`Composer::VOICES`] -- that stealing never actually
    /// happens -- was measured once by hand and then trusted for
    /// however long; a counter costs one increment on a path that runs
    /// a few times a minute, and `no_note_ever_has_to_take_a_voice_away`
    /// is what turns the claim into something the build checks.
    stolen: u32,
    /// Master gain for the fade in [`Phase::Fading`].
    fade: f32,

    /// One pole of low-pass across the whole output, and its
    /// coefficient.
    ///
    /// **Insurance rather than shaping.** The register and the reverb
    /// are where the squeal actually came from and both are fixed at
    /// the source; this is here so that no future note, harmonic or
    /// resonance can put anything sharp into a *background* layer
    /// again. Five kilohertz leaves every instrument in this file
    /// completely intact -- the highest partial anything produces is
    /// around fifteen hundred -- and takes the edge off anything that
    /// should not be there.
    tilt_left: f32,
    tilt_right: f32,
    tilt: f32,
}

impl Composer {
    /// Enough for three pad notes, a bass note and a couple of overlapping
    /// melody notes, several times over. Measured at eleven in the worst
    /// bar the generator produces; the rest is headroom so a stolen voice
    /// is a thing that never actually happens.
    const VOICES: usize = 24;

    pub fn new(rate: f32, seed: u64) -> Composer {
        Composer {
            rate,
            rng: Rng::new(seed),
            voices: [Voice::IDLE; Composer::VOICES],
            delay: Delay::new(rate),
            wanted: Mood::Menu,
            playing: Mood::Menu,
            phase: Phase::Resting,
            to_next_note: 0.0,
            to_next_drone: 0.0,
            to_next_centre: 0.0,
            centre: 0,
            piece_left: 0.0,
            phrase_motif: MOTIFS.len(),
            phrase_at: 0,
            phrase_octave: 2,
            phrase_left: 0,
            phrase_length: 0,
            last_freq: 0.0,
            same_note: 0,
            stolen: 0,
            // A few seconds before the first piece: the menu should
            // appear in silence and then be joined, not open with a
            // chord.
            rest_left: rate * 4.0,
            fade: 1.0,
            tilt_left: 0.0,
            tilt_right: 0.0,
            tilt: {
                let dt = 1.0 / rate;
                let rc = 1.0 / (std::f32::consts::TAU * 5_000.0);
                dt / (rc + dt)
            },
        }
    }

    /// What the game wants to hear. Cheap to call every frame with the
    /// same value.
    pub fn set_mood(&mut self, mood: Mood) {
        if mood == self.wanted {
            return;
        }
        self.wanted = mood;
        // A piece already sounding is faded out rather than cut. A rest
        // just gets shortened, so walking into a cave is answered within
        // a couple of seconds instead of after the minute of silence
        // that was already scheduled.
        match self.phase {
            Phase::Playing => self.phase = Phase::Fading,
            Phase::Resting => self.rest_left = self.rest_left.min(self.rate * 3.0),
            Phase::Fading => {}
        }
    }

    /// One stereo frame.
    #[inline]
    pub fn next(&mut self) -> (f32, f32) {
        let dt = 1.0 / self.rate;

        match self.phase {
            Phase::Resting => {
                self.rest_left -= 1.0;
                if self.rest_left <= 0.0 {
                    self.start_piece();
                }
            }
            Phase::Playing | Phase::Fading => {
                self.advance();
                if self.phase == Phase::Fading {
                    // Three seconds from full to nothing.
                    self.fade -= dt / 3.0;
                    if self.fade <= 0.0 {
                        // **Dropped rather than released here.** The
                        // master gain is already zero, so nothing is
                        // audible to click; and `silence` puts `fade`
                        // back to one, which would hand a voice that
                        // had just been faded out its full loudness
                        // again for the length of its release.
                        self.voices = [Voice::IDLE; Composer::VOICES];
                        self.silence();
                    }
                }
            }
        }

        // One noise sample per frame, shared by every voice that wants
        // one. They are all breathing the same air.
        let breath = self.rng.bipolar();
        let mut dry = 0.0;
        let mut left = 0.0;
        let mut right = 0.0;
        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }
            let pan = voice.pan;
            let sample = voice.next(dt, breath);
            // Equal-power-ish panning, done with the cheap linear
            // approximation: at these gains the difference from a real
            // cosine law is inaudible and this is inside the per-sample
            // loop.
            left += sample * (0.5 - pan * 0.5).sqrt();
            right += sample * (0.5 + pan * 0.5).sqrt();
            dry += sample;
        }

        let reverb = self.playing.recipe().reverb;
        let (wet_left, wet_right) = self.delay.run(dry * 0.5, reverb);
        // The delay is fed the mono sum and mixed back per side, so the
        // dry signal keeps the panning the voices were given and the
        // wet one spreads it.
        let gain = self.fade * 0.5;
        let out_left = (left + wet_left * 0.5) * gain;
        let out_right = (right + wet_right * 0.5) * gain;
        // See `tilt`.
        self.tilt_left += self.tilt * (out_left - self.tilt_left);
        self.tilt_right += self.tilt * (out_right - self.tilt_right);
        (self.tilt_left, self.tilt_right)
    }

    /// Stops scheduling, lets what is sounding die away, and books the
    /// next silence.
    ///
    /// **It used to switch the voices off**, which is what the comment
    /// at its one caller already claimed it did not do -- see
    /// [`Voice::release`] for the click that made and the number it
    /// measured.
    fn silence(&mut self) {
        for voice in &mut self.voices {
            voice.let_go(RELEASE);
        }
        self.phase = Phase::Resting;
        self.fade = 1.0;
        // Twenty-five seconds to a minute and a half. The long end is
        // long on purpose -- see the module note.
        self.rest_left = self.rate * self.rng.range(25.0, 90.0);
    }

    fn start_piece(&mut self) {
        self.playing = self.wanted;
        self.phase = Phase::Playing;
        self.fade = 1.0;
        self.centre = 0;
        // A minute to two and a half. In seconds rather than in bars,
        // because there are no bars -- and because how long a piece
        // lasts is a fact about how long somebody should be left alone
        // with it rather than a count of anything.
        self.piece_left = self.rate * self.rng.range(60.0, 150.0);
        let centre_gap = self.playing.recipe().centre_gap;
        self.to_next_centre = self.rate * self.rng.range(centre_gap.0, centre_gap.1);
        // A phrase left half-sung when the last piece ended does not
        // carry over into this one, whose chords mean something else.
        self.phrase_left = 0;
        // The drone first and immediately; the voice waits, so a piece
        // opens by fading in rather than by somebody starting to sing.
        self.to_next_drone = 0.0;
        self.to_next_note = self.rate * self.rng.range(4.0, 12.0);
    }

    /// One sample of the two clocks.
    ///
    /// Three counters that never divide into one another. That is the
    /// whole of what stops this being a metre: a note is due when its
    /// own clock runs out, and its own clock is reset to a fresh random
    /// gap every time -- so nothing in the music is ever a fixed
    /// distance from anything else in it.
    fn advance(&mut self) {
        self.piece_left -= 1.0;
        if self.piece_left <= 0.0 {
            // Let what is sounding ring out rather than cutting it --
            // the voices already have their own long releases, and
            // `silence` only stops scheduling new ones.
            self.silence();
            return;
        }

        self.to_next_centre -= 1.0;
        if self.to_next_centre <= 0.0 {
            let recipe = self.playing.recipe();
            // **Usually the next chord, and now and then the one after
            // it.** Always stepping by one means every piece walks the
            // same four chords in the same order for as long as it
            // lasts, which is a loop with a long period rather than no
            // loop -- and the one thing a listener does follow here is
            // where the harmony went.
            let step = if self.rng.chance(0.2) { 2 } else { 1 };
            self.centre = (self.centre + step) % recipe.progression.len();
            self.to_next_centre =
                self.rate * self.rng.range(recipe.centre_gap.0, recipe.centre_gap.1);
        }

        self.to_next_drone -= 1.0;
        if self.to_next_drone <= 0.0 {
            self.breathe();
        }

        self.to_next_note -= 1.0;
        if self.to_next_note <= 0.0 {
            self.sing();
        }
    }

    /// Renews one tone of the drone.
    ///
    /// **One at a time, never the whole chord.** Three tones started
    /// together and stopped together is a chord being struck, which is
    /// an event; three tones on their own schedules is a texture that
    /// happens to be harmonic, and the difference is most of what
    /// separates this from a keyboard.
    fn breathe(&mut self) {
        let recipe = self.playing.recipe();
        let gap = self.rng.range(recipe.drone_gap.0, recipe.drone_gap.1);
        self.to_next_drone = self.rate * gap;

        let root = recipe.progression[self.centre];
        // Root, fifth, or -- less often -- the third or the seventh.
        // The two plain ones carry the mood; the other two are what
        // stop it being a drone on one interval for ever.
        let degree = match self.rng.below(6) {
            0 | 1 => 0,
            2 | 3 => 4,
            4 => 2,
            _ => 6,
        };
        // **Two octaves lower than this used to be.** The roots are
        // already where a voice sits -- 220 Hz for the menu, 262 for
        // the day -- and the drone was being put two octaves *above*
        // them, so a background texture was singing at a kilohertz.
        // Forty per cent of the music's energy came out over 1 kHz,
        // which is exactly where the ear is most sensitive and exactly
        // what "it squeals" means.
        //
        // Below, at, and just above the root is where a pad belongs.
        let octave = match self.rng.below(5) {
            0 => -1,
            1 | 2 => 0,
            _ => 1,
        };
        let freq = self.degree_hz(&recipe, root + degree, octave);

        // Ten to thirty seconds, with a third of it spent arriving.
        // Nothing here has an attack a listener could point at.
        let life = self.rng.range(10.0, 30.0);
        // **The bottom octave gets the bass timbre and the centre of
        // the field.** Two sines a few cents apart is a lovely pad at
        // two hundred hertz and an unpleasant beat at fifty-five, and a
        // sub tone panned to one side is a sub tone half the listeners
        // cannot hear.
        let (timbre, gain, pan) = if octave < 0 {
            (Timbre::Bass, 0.13 * recipe.bass, 0.0)
        } else {
            // Drawn before the call: `note` takes `&mut self`, and the
            // generator lives on `self`.
            (Timbre::Pad, 0.10 * recipe.pad, self.rng.range(-0.8, 0.8))
        };
        self.note(timbre, freq, gain, life, life * 0.35, pan);
    }

    /// One sung note, or the next note of a phrase already under way.
    ///
    /// **One note per call, and the phrase is state.** Playing two or
    /// three at once would be a chord, which is an event with an
    /// attack; a phrase has to arrive one note at a time, at distances
    /// that are not multiples of each other.
    fn sing(&mut self) {
        let recipe = self.playing.recipe();

        if self.phrase_left == 0 {
            // Never the same phrase twice running. One re-roll rather
            // than a loop: two draws make a repeat rare, and a loop on
            // the audio thread that depends on a random number is a
            // loop with no bound on it.
            let mut motif = self.pick_motif(recipe.colours);
            if motif == self.phrase_motif {
                motif = self.pick_motif(recipe.colours);
            }
            self.phrase_motif = motif;
            // One octave over the drone and no more at the top: at
            // three this was landing above two kilohertz *before* its
            // harmonics, and the third harmonic of that is six -- a
            // whistle, not a voice.
            //
            // **The low one is new**, and it is the cheapest way a
            // melody has of answering itself: the same phrase said
            // again lower is a different thing said, where the same
            // phrase said again is a repeat. It matters more now that
            // `fold_into_register` exists, because the fold quietly
            // turns most of the top octave back into the middle one --
            // so without a bottom octave the sung line lived inside a
            // single one.
            self.phrase_octave = match self.rng.below(8) {
                0 | 1 => 0,
                2 | 3 => 2,
                _ => 1,
            };
            // Mostly one note on its own. A fragment now and then is
            // what keeps the hand-written material in the piece -- see
            // `MOTIFS` -- without any of it becoming a tune.
            self.phrase_left = if self.rng.chance(recipe.melody) {
                2 + self.rng.below(3) as u32
            } else {
                1
            };
            self.phrase_length = self.phrase_left;
            // **Where in the motif a fragment starts.** A single note
            // used to be the motif's *first* note, always, and singles
            // are the commonest event there is -- so the melody spent
            // its life on the handful of degrees the phrases happen to
            // begin on (over half of them a root or a fifth) and the
            // ends of the longer phrases were nearly never heard. A
            // note lifted out of the middle of a phrase is still that
            // phrase's material; starting every quotation at the top is
            // what made it sound like a short list.
            self.phrase_at = if self.phrase_left < MOTIFS[motif].notes.len() as u32 {
                self.rng
                    .below(MOTIFS[motif].notes.len() + 1 - self.phrase_left as usize)
            } else {
                0
            };
        }

        // The next thing in the motif that is not a rest. A rest inside
        // a fragment means nothing here: the gaps are already made of
        // rubato rather than of counted silence.
        let motif = MOTIFS[self.phrase_motif].notes;
        let mut found = None;
        while self.phrase_at < motif.len() {
            let (degree, length) = motif[self.phrase_at];
            self.phrase_at += 1;
            if degree != REST {
                found = Some((degree, length));
                break;
            }
        }
        let Some((degree, length)) = found else {
            // Ran off the end of the motif. Start again next time.
            self.phrase_left = 0;
            self.to_next_note =
                self.rate * self.rng.range(recipe.note_gap.0, recipe.note_gap.1);
            return;
        };
        self.phrase_left -= 1;

        let root = recipe.progression[self.centre];
        let mut freq = fold_into_register(self.degree_hz(&recipe, root + degree, self.phrase_octave));
        // **A melody may repeat a note; it may not settle on one.** Two
        // in a row is a phrase doing it on purpose -- one of the motifs
        // is a bell, and strikes the same note twice. Three is the
        // chord, the motif and the register fold agreeing by accident,
        // and it happens: measured before this, `Night` sang one note
        // five times running. The third one is moved an octave, which
        // keeps the note the phrase asked for and makes it a different
        // thing to hear.
        if (freq - self.last_freq).abs() < 0.5 {
            self.same_note += 1;
        } else {
            self.same_note = 0;
        }
        if self.same_note >= 2 {
            freq = if freq * 2.0 <= LEAD_CEILING {
                freq * 2.0
            } else {
                freq * 0.5
            };
            self.same_note = 0;
        }
        self.last_freq = freq;
        // Long, and longer than the gap to the next one, so the notes
        // of a phrase overlap rather than queueing.
        let life = self.rng.range(2.5, 5.5) * (length as f32 * 0.25 + 0.75);
        // Nearly a second to arrive. A note that starts suddenly is a
        // note somebody pressed.
        let attack = self.rng.range(0.5, 1.1);
        let pan = self.rng.range(-0.35, 0.35);
        // Drawn before the call for the reason `breathe` gives: `note`
        // takes `&mut self` and so does the generator.
        let gain = self.phrase_gain(freq);
        self.note(Timbre::Lead, freq, gain, life, attack, pan);

        // Soon if the phrase goes on, a long way off if it does not --
        // and a fresh random distance either way.
        //
        // **The written length decides the distance now.** It used to
        // decide only how long the note rang, and the gap to the next
        // one was the same 1.2-to-2.8 seconds whether the motif said to
        // hold that note for one unit or for six -- so the rhythms in
        // [`MOTIFS`] were, in the only sense a listener can hear,
        // not there. Half a second to a second per written unit, drawn
        // fresh each time: a motif's shape survives and its timing is
        // still nobody's idea of a beat, which is what
        // `the_music_has_no_beat` is there to keep true.
        self.to_next_note = self.rate
            * if self.phrase_left > 0 {
                self.rng.range(0.55, 1.05) * length as f32
            } else {
                self.rng.range(recipe.note_gap.0, recipe.note_gap.1)
            };
    }

    /// A phrase from the palette this mood is allowed to draw on.
    ///
    /// Two passes over a table of twenty-odd entries rather than
    /// building a list of candidates: this runs a few times a minute on
    /// the audio thread, where the rule is not "be fast" but "do not
    /// allocate".
    fn pick_motif(&mut self, colours: &[Colour]) -> usize {
        let matching = MOTIFS.iter().filter(|m| colours.contains(&m.colour)).count();
        if matching == 0 {
            // Unreachable while every mood names a colour that exists,
            // and a silent melody would be the wrong way to find out.
            return self.rng.below(MOTIFS.len());
        }
        let mut nth = self.rng.below(matching);
        for (i, motif) in MOTIFS.iter().enumerate() {
            if colours.contains(&motif.colour) {
                if nth == 0 {
                    return i;
                }
                nth -= 1;
            }
        }
        0
    }

    /// How loud this note of the phrase is.
    ///
    /// **Every sung note used to be exactly 0.130.** Measured over
    /// twenty minutes of each mood, the loudest note and the quietest
    /// were the same number to three decimal places -- which is not
    /// something a person playing an instrument can do even when trying
    /// to, and the ear notices the absence of variation long before it
    /// could name it. Three things vary it, and each one is a thing a
    /// player does:
    ///
    /// * a phrase leans on its first note and gives way towards its
    ///   last;
    /// * no two notes are quite equal;
    /// * and a note high in the register is pulled back, because the
    ///   ear hears the top of this range as louder than the bottom at
    ///   equal power and the music is supposed to stay behind the game.
    fn phrase_gain(&mut self, freq: f32) -> f32 {
        let spot = if self.phrase_length > 1 {
            (self.phrase_length - self.phrase_left - 1) as f32 / (self.phrase_length - 1) as f32
        } else {
            // A note on its own is neither the start of anything nor
            // the end of it.
            0.4
        };
        let contour = 1.0 - 0.35 * spot;
        // The constants hold the *average* where it has always been --
        // a sung note used to be 0.130 flat, and across a mood these
        // still average near it. What is new is that no two of them
        // are the same number.
        let register = (650.0 / freq.max(120.0)).clamp(0.7, 1.05);
        0.17 * contour * register * self.rng.range(0.82, 1.18)
    }

    /// Turns a scale degree into a frequency.
    ///
    /// Degrees run off both ends of the scale and wrap into the next
    /// octave, which is what makes a motif written as `[0, 2, 4]` mean
    /// the same shape wherever its chord puts it. The `rem_euclid` is
    /// the whole reason negative degrees work: `-1 % 7` is `-1` in Rust
    /// and the sixth degree an octave down is what a musician means.
    fn degree_hz(&self, recipe: &Recipe, degree: i32, octave: i32) -> f32 {
        let len = recipe.scale.len() as i32;
        let index = degree.rem_euclid(len) as usize;
        let octaves = octave + degree.div_euclid(len);
        let semitones = recipe.scale[index] + octaves * 12;
        recipe.root * 2f32.powf(semitones as f32 / 12.0)
    }

    /// Starts a note, stealing the oldest voice if every one is busy.
    fn note(
        &mut self,
        timbre: Timbre,
        freq: f32,
        gain: f32,
        life: f32,
        attack: f32,
        pan: f32,
    ) {
        let free = self.voices.iter().position(|v| !v.active);
        let slot = match free {
            Some(slot) => slot,
            None => {
                // Whichever is furthest through its envelope: the one
                // whose disappearance is least likely to be noticed.
                // Counted, because taking a sounding voice away is a
                // step in the output -- see `stolen`.
                self.stolen += 1;
                let mut oldest = 0;
                for (i, v) in self.voices.iter().enumerate() {
                    if v.age / v.life.max(1e-4)
                        > self.voices[oldest].age / self.voices[oldest].life.max(1e-4)
                    {
                        oldest = i;
                    }
                }
                oldest
            }
        };
        self.voices[slot] = Voice {
            active: true,
            timbre,
            freq,
            // Not zero: every note starting at the same phase makes the
            // chord's attack a single click.
            phase: self.rng.unit(),
            phase_b: self.rng.unit(),
            phase_c: self.rng.unit(),
            gain,
            age: 0.0,
            life,
            attack: attack.max(0.001),
            pan: pan.clamp(-1.0, 1.0),
            release: 1.0,
            release_rate: 0.0,
        };
    }

    /// Renders `seconds` of one mood into interleaved stereo, for
    /// `--export-sounds`.
    ///
    /// Starts the piece immediately rather than waiting out the opening
    /// rest, because an exported file of forty seconds of silence
    /// followed by two bars is not what anybody asked for.
    pub fn render(mood: Mood, seconds: f32, rate: u32, seed: u64) -> Vec<f32> {
        let mut composer = Composer::new(rate as f32, seed);
        composer.wanted = mood;
        composer.start_piece();
        let frames = (seconds * rate as f32) as usize;
        let mut out = Vec::with_capacity(frames * 2);
        for _ in 0..frames {
            // A piece that ends inside the window is followed by the
            // silence the game would give it, which is honest about what
            // the music does. The rest is capped so the file is not
            // mostly nothing.
            if composer.phase == Phase::Resting {
                composer.rest_left = composer.rest_left.min(rate as f32 * 6.0);
            }
            let (l, r) = composer.next();
            out.push(l);
            out.push(r);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 22_050.0;

    /// The rate the note-counting tests run the composer at.
    ///
    /// **A thousand samples a second, and it is the same music.** Every
    /// clock in the scheduler is set in seconds and converted by the
    /// rate, and every rule that decides a note is the same rule, so
    /// what comes out at 1 kHz is the same music in every sense a
    /// statistic can see. It is not the same *draw* -- one sample of
    /// noise is taken per frame, so a different rate walks the
    /// generator at a different speed and the piece is a different one
    /// of the pieces this composer writes -- which is exactly why the
    /// tests below ask about distributions and none of them asserts a
    /// particular note.
    ///
    /// It buys twenty-two times as much music per second of test, which
    /// is the difference between measuring ten minutes of every mood
    /// and measuring twenty seconds of three.
    const SCHEDULE_RATE: f32 = 1_000.0;

    /// **A range rather than one number.** The rule used to be that
    /// every motif totalled exactly eight units, which was a bar's
    /// worth back when there were bars. Nothing reads a motif's total
    /// any more, so all that rule still bought was every phrase in the
    /// game being the same size of gesture -- and a set of phrases that
    /// are all the same length is one of the ways an evening of this
    /// starts sounding like a list. What is still worth enforcing is
    /// that a phrase is a phrase: long enough to be one, short enough
    /// not to be a tune.
    #[test]
    fn no_motif_is_a_fragment_or_a_tune() {
        for (i, motif) in MOTIFS.iter().enumerate() {
            let total: u32 = motif.notes.iter().map(|(_, length)| *length).sum();
            assert!(
                (MOTIF_MIN_UNITS..=MOTIF_MAX_UNITS).contains(&total),
                "motif {i} is {total} units long"
            );
            assert!(!motif.notes.is_empty(), "motif {i} is empty");
            assert!(
                motif.notes.iter().any(|(degree, _)| *degree != REST),
                "motif {i} is nothing but rests"
            );
        }
    }

    /// ...and they are not all the same length, which is the half of
    /// the old rule that was doing the damage.
    #[test]
    fn the_motifs_are_not_all_cut_to_one_length() {
        let mut lengths: Vec<u32> = MOTIFS
            .iter()
            .map(|m| m.notes.iter().map(|(_, l)| *l).sum())
            .collect();
        lengths.sort_unstable();
        lengths.dedup();
        assert!(
            lengths.len() >= 5,
            "every phrase in the game is one of {} lengths: {lengths:?}",
            lengths.len()
        );
    }

    /// **Where a phrase stops is what a listener remembers of it.**
    /// Three of the original six ended on the same degree, by the same
    /// descending step, and all six ended on a note of the chord -- so
    /// a piece of them was a row of small conclusions, whichever order
    /// they came in.
    #[test]
    fn the_motifs_do_not_all_come_home_the_same_way() {
        let endings: Vec<i32> = MOTIFS
            .iter()
            .filter_map(|m| m.notes.iter().rev().find(|(d, _)| *d != REST))
            .map(|(d, _)| *d)
            .collect();
        for degree in &endings {
            let same = endings.iter().filter(|d| *d == degree).count();
            assert!(
                same * 2 <= endings.len(),
                "{same} of {} phrases end on degree {degree}",
                endings.len()
            );
        }
        // ...and some of them do not resolve at all. A degree that is
        // not 0, 2 or 4 is not in the chord the phrase is sitting on.
        let open = endings
            .iter()
            .filter(|d| !matches!(d.rem_euclid(7), 0 | 2 | 4))
            .count();
        assert!(open >= 3, "only {open} phrases end on a question");
    }

    /// Two phrases with the same rhythm are the same phrase to anybody
    /// not listening closely, which is everybody: this is background
    /// music.
    #[test]
    fn no_two_motifs_share_a_rhythm() {
        let mut rhythms: Vec<Vec<u32>> = MOTIFS
            .iter()
            .map(|m| m.notes.iter().map(|(_, l)| *l).collect())
            .collect();
        let total = rhythms.len();
        rhythms.sort();
        rhythms.dedup();
        assert_eq!(total, rhythms.len(), "two motifs are the same rhythm");
    }

    /// Silence inside a phrase is how a phrase breathes, and one motif
    /// in six having any was not enough for it ever to be heard.
    #[test]
    fn enough_motifs_have_a_rest_in_them_to_be_noticed() {
        let resting = MOTIFS
            .iter()
            .filter(|m| m.notes.iter().any(|(d, _)| *d == REST))
            .count();
        assert!(
            resting * 6 >= MOTIFS.len(),
            "only {resting} of {} phrases have a rest",
            MOTIFS.len()
        );
    }

    #[test]
    fn every_mood_has_a_playable_recipe() {
        for &mood in Mood::ALL {
            let recipe = mood.recipe();
            assert!(!recipe.scale.is_empty());
            assert!(!recipe.progression.is_empty());
            assert!(recipe.note_gap.0 > 0.5 && recipe.note_gap.1 > recipe.note_gap.0);
            assert!(recipe.drone_gap.0 > 0.5 && recipe.drone_gap.1 > recipe.drone_gap.0);
            assert!(recipe.centre_gap.0 > 5.0 && recipe.centre_gap.1 > recipe.centre_gap.0);
            // **The drone has to outlast the singing.** If tones were
            // renewed faster than notes arrive, the texture would be
            // the busy layer and the voice the background, which is the
            // arrangement upside down.
            assert!(
                recipe.drone_gap.0 >= recipe.note_gap.0,
                "{mood:?} renews its drone faster than it sings"
            );
            // ...and the harmony has to outlast the drone, for the same
            // reason one step up.
            assert!(
                recipe.centre_gap.0 >= recipe.drone_gap.0,
                "{mood:?} changes chord faster than it renews its drone"
            );
            assert!(recipe.root > 20.0);
            // Every chord of the progression has to exist in the scale
            // it is a degree of: a pentatonic mood with a chord on
            // degree 6 is a typo that would only ever be heard as the
            // melody being in the wrong place.
            for &degree in recipe.progression {
                assert!(
                    (degree as usize) < recipe.scale.len(),
                    "{mood:?} roots a chord on degree {degree} of a \
                     {}-note scale",
                    recipe.scale.len()
                );
            }
        }
    }

    /// **A mood nothing can play is a mood that plays everything.** A
    /// palette naming a colour no motif has would silently fall back to
    /// the whole table, and the mood would quietly stop being itself.
    #[test]
    fn every_mood_can_draw_on_enough_phrases_to_vary() {
        for &mood in Mood::ALL {
            let recipe = mood.recipe();
            let available = MOTIFS
                .iter()
                .filter(|m| recipe.colours.contains(&m.colour))
                .count();
            assert!(
                available >= 6,
                "{mood:?} has only {available} phrases to choose from"
            );
        }
    }

    /// Two moods that can play the same phrases over the same scale are
    /// one mood with two names -- which is what the soundscape was
    /// doing with rain and a thunderstorm before either had a recipe of
    /// its own.
    #[test]
    fn no_two_moods_are_the_same_music() {
        for (i, &one) in Mood::ALL.iter().enumerate() {
            for &other in &Mood::ALL[i + 1..] {
                let a = one.recipe();
                let b = other.recipe();
                let same_palette = a.colours.len() == b.colours.len()
                    && a.colours.iter().all(|c| b.colours.contains(c));
                let same_harmony = a.scale == b.scale
                    && a.progression == b.progression
                    && (a.root - b.root).abs() < 0.01;
                assert!(
                    !(same_palette && same_harmony),
                    "{one:?} and {other:?} are the same piece of music"
                );
                assert!(
                    !same_palette || a.scale != b.scale,
                    "{one:?} and {other:?} share both a scale and a palette"
                );
            }
        }
    }

    /// Degrees are written relative to a chord root and routinely go
    /// negative. Rust's `%` would put those below the tonic *and* in the
    /// wrong octave.
    #[test]
    fn negative_degrees_land_an_octave_down() {
        let composer = Composer::new(RATE, 1);
        let recipe = Mood::Day.recipe();
        let tonic = composer.degree_hz(&recipe, 0, 2);
        let octave_up = composer.degree_hz(&recipe, recipe.scale.len() as i32, 2);
        assert!((octave_up / tonic - 2.0).abs() < 0.001);
        let octave_down = composer.degree_hz(&recipe, -(recipe.scale.len() as i32), 2);
        assert!((tonic / octave_down - 2.0).abs() < 0.001);
    }

    /// The one thing that would be unforgivable on the audio thread: a
    /// sample that is not a number, or one loud enough to hurt. Run over
    /// a minute of every mood, which is several whole pieces.
    #[test]
    fn nothing_a_mood_produces_is_out_of_range() {
        for mood in [Mood::Menu, Mood::Cave, Mood::Storm, Mood::Sea, Mood::Peril] {
            let rendered = Composer::render(mood, 60.0, 22_050, 42);
            assert!(rendered.iter().all(|s| s.is_finite()), "{mood:?} produced a NaN");
            let peak = rendered.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak < 1.0, "{mood:?} peaked at {peak}");
            assert!(peak > 0.05, "{mood:?} never made a sound");
        }
    }

    /// How strongly a rendered stretch of music implies a **beat**.
    ///
    /// The amplitude envelope, autocorrelated over the lags a tempo
    /// lives at. Music with a metre repeats its loudness pattern at the
    /// bar and scores high; a texture whose events are at unrelated
    /// distances has nothing to repeat and scores low. It is the
    /// standard way of asking the question and it asks exactly the one
    /// that matters here.
    fn beat_strength(rendered: &[f32], rate: f32) -> f32 {
        // Rectify, then a one-pole at 8 Hz: what is left is how loud
        // the music is over time, with the waveform itself gone.
        let dt = 1.0 / rate;
        let rc = 1.0 / (std::f32::consts::TAU * 8.0);
        let alpha = dt / (rc + dt);
        let mut level = 0.0f32;
        // Every hundredth frame is plenty for something bandlimited to
        // 8 Hz, and it turns a minute of audio into a few hundred
        // numbers.
        let stride = (rate / 100.0) as usize;
        let mut envelope = Vec::with_capacity(rendered.len() / (2 * stride) + 1);
        for (i, frame) in rendered.chunks(2).enumerate() {
            let mono = (frame[0] + frame.get(1).copied().unwrap_or(0.0)) * 0.5;
            level += alpha * (mono.abs() - level);
            if i % stride == 0 {
                envelope.push(level);
            }
        }
        if envelope.len() < 64 {
            return 0.0;
        }

        // **The rise, not the level.** Autocorrelating loudness itself
        // measures smoothness: a drone fading over twenty seconds
        // correlates almost perfectly with itself a quarter of a second
        // later, and scores as metrical as a drum machine. What a beat
        // actually is, is *onsets* at regular distances -- so what gets
        // correlated is the half-wave rectified difference, which is
        // large where something starts and zero where nothing does.
        let onsets: Vec<f32> = envelope
            .windows(2)
            .map(|w| (w[1] - w[0]).max(0.0))
            .collect();
        if onsets.len() < 64 {
            return 0.0;
        }
        let mean = onsets.iter().sum::<f32>() / onsets.len() as f32;
        let centred: Vec<f32> = onsets.iter().map(|v| v - mean).collect();
        let energy: f32 = centred.iter().map(|v| v * v).sum();
        // Nothing starts sharply enough to be an onset at all, which is
        // itself the answer: there is nothing here to tap along to.
        if energy < 1e-9 {
            return 0.0;
        }

        // A quarter of a second to two and a half: every tempo anybody
        // would tap along to.
        let per_second = 100.0;
        let low = (0.25 * per_second) as usize;
        let high = ((2.5 * per_second) as usize).min(centred.len() / 2);
        let mut best = 0.0f32;
        for lag in low..high {
            let dot: f32 = centred
                .iter()
                .zip(centred[lag..].iter())
                .map(|(a, b)| a * b)
                .sum();
            best = best.max(dot / energy);
        }
        best
    }

    /// **The regression test for "it sounds arcade".**
    ///
    /// The timbre was fixed first -- the lead was a triangle wave, which
    /// is an NES -- and it was not enough, because what makes music read
    /// as a video game is not the sound of the notes. It is that there
    /// are **bars**: eighth notes on a grid, a bass note on every
    /// downbeat, four-bar chords. A tune playing while somebody chops
    /// wood is a game announcing itself however carefully the notes were
    /// chosen.
    ///
    /// So there is no tempo in this file at all now, and this is what
    /// says so. Three counters that never divide into one another
    /// cannot produce a pulse, and a listener cannot tap along to
    /// something that has none.
    #[test]
    fn the_music_has_no_beat() {
        // **Against a reference rather than an absolute line.** The
        // measure is a correlation over a handful of events -- four
        // minutes of this produces a few dozen -- and a small sample
        // will always find *some* lag where two of them happen to line
        // up. What is not arguable is the comparison: a piece with a
        // metre scores several times what a piece without one does.
        let pulse = beat_strength(&metronome(22_050.0, 240.0, 0.5), 22_050.0);
        for mood in [Mood::Menu, Mood::Day, Mood::Storm, Mood::Peril] {
            let rendered = Composer::render(mood, 240.0, 22_050, 42);
            let beat = beat_strength(&rendered, 22_050.0);
            assert!(
                beat < pulse * 0.6,
                "{mood:?} pulses at {beat:.2} against a metronome's {pulse:.2} --                  that is a metre"
            );
        }
    }

    /// A click track, for the test above to measure itself against.
    fn metronome(rate: f32, seconds: f32, period: f32) -> Vec<f32> {
        let frames = (rate * seconds) as usize;
        let mut out = vec![0.0f32; frames * 2];
        let step = (rate * period) as usize;
        let click = (rate * 0.04) as usize;
        for frame in 0..frames {
            if frame % step < click {
                out[frame * 2] = 0.5;
                out[frame * 2 + 1] = 0.5;
            }
        }
        out
    }

    /// ...and the measure can hear one when there is one, or the test
    /// above passes by measuring nothing.
    #[test]
    fn the_beat_measure_knows_a_pulse_when_it_hears_one() {
        // Two beats a second, which is the sort of thing the old
        // scheduler produced.
        let rate = 22_050.0;
        let beat = beat_strength(&metronome(rate, 20.0, 0.5), rate);
        assert!(
            beat > 0.6,
            "the measure could not hear a pulse two a second: {beat:.2}"
        );
    }

    /// What fraction of a rendered stretch's energy sits above
    /// `cutoff`.
    ///
    /// Two poles, so the split is steep enough to mean something.
    fn energy_above(rendered: &[f32], rate: f32, cutoff: f32) -> f32 {
        let dt = 1.0 / rate;
        let rc = 1.0 / (std::f32::consts::TAU * cutoff);
        let alpha = rc / (rc + dt);
        let (mut in_a, mut out_a) = (0.0f32, 0.0f32);
        let (mut in_b, mut out_b) = (0.0f32, 0.0f32);
        let (mut high, mut total) = (0.0f64, 0.0f64);
        for frame in rendered.chunks(2) {
            let v = (frame[0] + frame.get(1).copied().unwrap_or(0.0)) * 0.5;
            let first = alpha * (out_a + v - in_a);
            in_a = v;
            out_a = first;
            let second = alpha * (out_b + first - in_b);
            in_b = first;
            out_b = second;
            high += (second * second) as f64;
            total += (v * v) as f64;
        }
        if total < 1e-12 {
            return 0.0;
        }
        (high / total) as f32
    }

    /// **The regression test for "it squeals".**
    ///
    /// Music is a *background* layer, and a background layer has no
    /// business putting energy where the ear is sharpest -- which is
    /// two to five kilohertz, the band a scream is in and the band a
    /// smoke alarm is tuned to.
    ///
    /// It was doing exactly that. The roots are already in the register
    /// a voice sits in (220 Hz for the menu, 262 for the day) and the
    /// drone was being placed two octaves *above* them, so a texture
    /// nobody was supposed to be listening to was singing at a
    /// kilohertz -- and the lead an octave over that, with harmonics
    /// over that again. Measured on the exported files:
    ///
    /// ```text
    ///                 above 1 kHz   above 2 kHz
    ///   music.day          41%           15%
    ///   music.menu         30%           10%
    /// ```
    ///
    /// Three things fixed it and this holds all three: the register
    /// came down two octaves, the delay's feedback got the treble
    /// damping every real reflection has, and the lead's harmonics now
    /// fade as it climbs, the way a wind instrument's do.
    #[test]
    fn the_music_does_not_squeal() {
        for &mood in Mood::ALL {
            let rendered = Composer::render(mood, 60.0, 22_050, 7);
            let sharp = energy_above(&rendered, 22_050.0, 2_000.0);
            let high = energy_above(&rendered, 22_050.0, 1_000.0);
            assert!(
                sharp < 0.04,
                "{mood:?} puts {:.0}% of itself above 2 kHz -- that is a whistle",
                sharp * 100.0
            );
            assert!(
                high < 0.20,
                "{mood:?} puts {:.0}% of itself above 1 kHz",
                high * 100.0
            );
        }
    }

    /// ...and it is still music rather than a rumble: most of it has to
    /// be somewhere a note can be heard at all.
    #[test]
    fn the_music_is_not_all_bass_either() {
        let rendered = Composer::render(Mood::Day, 60.0, 22_050, 7);
        let audible = energy_above(&rendered, 22_050.0, 120.0);
        assert!(
            audible > 0.2,
            "only {:.0}% of the music is above 120 Hz -- that is a rumble",
            audible * 100.0
        );
    }

    /// Every note the melody actually sings, over `seconds` of one
    /// mood, as (frequency, loudness).
    ///
    /// **Read off the voices rather than off the scheduler.** What is
    /// interesting is what a listener would hear, and between the
    /// scheduler and the listener sit the register fold, the phrase
    /// contour and voice stealing -- all three of which are things that
    /// have been wrong. A note is new when its age is still one frame,
    /// which also catches a voice that was taken from somebody else.
    fn sung(mood: Mood, seconds: f32, seed: u64) -> Vec<(f32, f32)> {
        let mut composer = Composer::new(SCHEDULE_RATE, seed);
        composer.wanted = mood;
        composer.start_piece();
        let dt = 1.0 / SCHEDULE_RATE;
        let mut notes = Vec::new();
        for _ in 0..(seconds * SCHEDULE_RATE) as usize {
            // The rests are real and they are long; shortened here so a
            // measurement is of music rather than mostly of silence.
            if composer.phase == Phase::Resting {
                composer.rest_left = composer.rest_left.min(SCHEDULE_RATE * 6.0);
            }
            composer.next();
            for voice in composer.voices.iter() {
                if voice.active && voice.timbre == Timbre::Lead && voice.age <= dt * 1.5 {
                    notes.push((voice.freq, voice.gain));
                }
            }
        }
        notes
    }

    /// The longest stretch of the melody that is one note repeated.
    fn longest_run(notes: &[f32]) -> usize {
        let mut best = 0;
        let mut run = 0;
        for (i, note) in notes.iter().enumerate() {
            if i > 0 && (note - notes[i - 1]).abs() < 0.01 {
                run += 1;
            } else {
                run = 1;
            }
            best = best.max(run);
        }
        best
    }

    /// The longest run of notes that is immediately repeated note for
    /// note -- the melodic equivalent of a loop point.
    fn longest_immediate_repeat(notes: &[f32]) -> usize {
        let mut best = 0;
        for len in 1..=notes.len() / 2 {
            for i in 0..notes.len().saturating_sub(2 * len) {
                if (0..len).all(|k| (notes[i + k] - notes[i + len + k]).abs() < 0.01) {
                    best = best.max(len);
                    break;
                }
            }
        }
        best
    }

    /// **The melody has to have somewhere to go.** A generator that
    /// leans on one note is the classic failure of this kind of music,
    /// and it does not announce itself in a spectrum or a beat measure
    /// -- it has to be counted.
    ///
    /// Measured over twenty minutes of each mood, before this pass and
    /// after it -- the entry points into the motifs, the low octave and
    /// the wider table together:
    ///
    /// ```text
    ///              distinct pitches   commonest note   longest run
    ///   Menu            16 -> 19         14% -> 12%       4 -> 2
    ///   Day             18 -> 16         15% -> 17%       3 -> 2
    ///   Night           17 -> 18         15% -> 13%       3 -> 2
    ///   Cave            12 -> 14         22% -> 21%       4 -> 2
    ///   Rain            17 -> 19         18% -> 10%       3 -> 2
    ///   Peril           18 -> 20         15% -> 11%       2 -> 2
    /// ```
    ///
    /// `Day` is the one that fell, and it fell for a good reason: it
    /// used to reach 2637 Hz, and half of what it counted as vocabulary
    /// was the same seven notes in an octave nothing should have been
    /// singing in.
    #[test]
    fn a_melody_does_not_lean_on_one_note() {
        for &mood in Mood::ALL {
            let mut notes = Vec::new();
            for seed in [1u64, 7, 42, 1234] {
                notes.extend(sung(mood, 600.0, seed).into_iter().map(|(f, _)| f));
            }
            assert!(notes.len() > 40, "{mood:?} barely sang at all");

            let mut distinct: Vec<i32> = notes.iter().map(|f| (f * 10.0) as i32).collect();
            distinct.sort_unstable();
            distinct.dedup();
            assert!(
                distinct.len() >= 12,
                "{mood:?} has only {} notes in its vocabulary",
                distinct.len()
            );

            let commonest = distinct
                .iter()
                .map(|d| notes.iter().filter(|f| (**f * 10.0) as i32 == *d).count())
                .max()
                .unwrap_or(0);
            // A quarter. The two moods that come closest are the two
            // with the smallest palettes on purpose -- a cave and a
            // pentatonic wood, both a little over a fifth.
            assert!(
                commonest * 4 <= notes.len(),
                "{mood:?} spends {:.0}% of its melody on one note",
                100.0 * commonest as f32 / notes.len() as f32
            );

            // ...and it does not stand still, either. Two in a row is
            // a phrase repeating a note, which is a thing music does
            // and one of the motifs does on purpose; three is the
            // guard in `sing` having failed. Before that guard, `Night`
            // managed five.
            let run = longest_run(&notes);
            assert!(run < 4, "{mood:?} sang the same note {run} times running");
        }
    }

    /// **Nothing in the melody repeats itself immediately.** Two
    /// identical phrases back to back is the one pattern that gives a
    /// generator away instantly -- a listener who has heard a thing
    /// once is *listening* for it the second time.
    #[test]
    fn no_phrase_is_sung_twice_in_a_row() {
        for &mood in Mood::ALL {
            let notes: Vec<f32> = sung(mood, 900.0, 5).into_iter().map(|(f, _)| f).collect();
            let repeat = longest_immediate_repeat(&notes);
            assert!(
                repeat < 4,
                "{mood:?} sang {repeat} notes and then sang them again"
            );
        }
    }

    /// **A note that is always exactly as loud as the last one is a
    /// machine.** Every sung note used to be gain 0.130 exactly, in
    /// every mood, for the whole life of the file.
    #[test]
    fn no_two_notes_are_struck_with_the_same_weight() {
        for &mood in Mood::ALL {
            let gains: Vec<f32> = sung(mood, 600.0, 11).into_iter().map(|(_, g)| g).collect();
            assert!(gains.len() > 20, "{mood:?} barely sang at all");
            let low = gains.iter().fold(f32::MAX, |m, g| m.min(*g));
            let high = gains.iter().fold(0.0f32, |m, g| m.max(*g));
            let mean = gains.iter().sum::<f32>() / gains.len() as f32;
            assert!(
                high - low > mean * 0.3,
                "{mood:?} plays every note within {:.1}% of the same loudness",
                100.0 * (high - low) / mean
            );
            // ...and not by being wild about it: a background layer
            // that jumps by a factor of four is a layer somebody keeps
            // turning down.
            assert!(
                high < low * 4.0,
                "{mood:?} ranges from {low:.3} to {high:.3}"
            );
        }
    }

    /// **The sung line stays in the register it was written for.** A
    /// degree is the chord's root plus the motif's offset plus the
    /// phrase's octave, and before the fold in `fold_into_register`
    /// those three added up to notes at 2637 Hz -- an octave and a half
    /// above anything the timbre was designed for.
    #[test]
    fn the_melody_never_climbs_out_of_its_own_range() {
        for &mood in Mood::ALL {
            for seed in [3u64, 21] {
                for (freq, _) in sung(mood, 600.0, seed) {
                    assert!(
                        (LEAD_FLOOR..=LEAD_CEILING).contains(&freq),
                        "{mood:?} sang at {freq:.0} Hz"
                    );
                }
            }
        }
    }

    /// ...and the fold that keeps it there moves notes by octaves, so
    /// what comes out is still the note that was asked for.
    #[test]
    fn folding_a_note_into_the_register_keeps_its_pitch_class() {
        for hz in [55.0f32, 130.81, 261.63, 1046.5, 2637.0, 4186.0] {
            let folded = fold_into_register(hz);
            assert!((LEAD_FLOOR..=LEAD_CEILING).contains(&folded));
            let octaves = (folded / hz).log2();
            assert!(
                (octaves - octaves.round()).abs() < 1e-4,
                "{hz} became {folded}, which is {octaves} octaves"
            );
        }
        // A frequency that could not be a note at all must not spin the
        // loop for ever.
        assert_eq!(fold_into_register(0.0), LEAD_FLOOR);
        assert_eq!(fold_into_register(f32::NAN), LEAD_FLOOR);
    }

    /// The claim above `Composer::VOICES` -- that the array is deep
    /// enough that a note never has to take a voice from another note
    /// -- measured rather than remembered. Taking one is a step in the
    /// output, which is a click.
    #[test]
    fn no_note_ever_has_to_take_a_voice_away() {
        for &mood in Mood::ALL {
            let mut composer = Composer::new(SCHEDULE_RATE, 77);
            composer.wanted = mood;
            composer.start_piece();
            for _ in 0..(SCHEDULE_RATE as usize * 900) {
                if composer.phase == Phase::Resting {
                    composer.rest_left = composer.rest_left.min(SCHEDULE_RATE * 4.0);
                }
                composer.next();
            }
            assert_eq!(composer.stolen, 0, "{mood:?} ran out of voices");
        }
    }

    /// How loud the music still was in the last ten milliseconds
    /// before it stopped, as a share of its loudest ten milliseconds.
    ///
    /// **Three measures were tried and two of them cannot tell a cut
    /// from music.**
    ///
    /// * *The largest sample-to-sample step* is the obvious one and it
    ///   is useless: a sine at 900 Hz and a tenth of full scale moves
    ///   by a fifth of its own amplitude between two samples at 22 kHz
    ///   all by itself. Measured, the cut scored 32% of peak and the
    ///   honest version 24% -- the same number.
    /// * *The largest fall in level between two ten-millisecond
    ///   windows* is no better, because two detuned pad tones beating
    ///   against each other genuinely halve the level in that time.
    ///   Measured: 51% against 37%, and the largest falls in the fixed
    ///   version were at 104 seconds, in the middle of a piece, with
    ///   nothing wrong at all.
    /// * What a cut actually is, is the music being *at some level* and
    ///   then at none, with nothing in between. So: of all the windows
    ///   immediately followed by silence, how loud was the loudest?
    ///   Beating never reaches silence, and a piece that dies away
    ///   properly is already at nothing when it gets there.
    fn level_before_silence(rendered: &[f32], rate: f32) -> f32 {
        let window = (rate * 0.01) as usize;
        let mut levels = Vec::with_capacity(rendered.len() / (2 * window) + 1);
        for chunk in rendered.chunks(2 * window) {
            let sum: f64 = chunk.iter().map(|s| (*s as f64) * (*s as f64)).sum();
            levels.push((sum / chunk.len() as f64).sqrt() as f32);
        }
        let peak = levels.iter().fold(0.0f32, |m, l| m.max(*l));
        if peak <= 0.0 {
            return 0.0;
        }
        let mut worst = 0.0f32;
        for pair in levels.windows(2) {
            if pair[1] < peak * 0.02 {
                worst = worst.max(pair[0]);
            }
        }
        worst / peak
    }

    /// **A piece has to end by dying away rather than by stopping.**
    /// `silence` used to switch every voice off where it stood -- with
    /// two or three drone tones halfway through thirty-second
    /// envelopes, which is a cut, at the one moment the music was
    /// supposed to be disappearing.
    ///
    /// Measured over four minutes of each mood, before the release
    /// ramp and after -- how loud the last ten milliseconds before a
    /// silence were, against the loudest ten milliseconds in the
    /// piece:
    ///
    /// ```text
    ///   Night   21% -> 4%
    ///   Cave    11% -> 3%
    ///   Peril   27% -> 5%
    /// ```
    ///
    /// The moods whose pieces happened to end on a quiet bar scored
    /// 4% even with the cut in place, which is the other reason this
    /// runs over several of them: a click is a thing that happens when
    /// something loud is sounding, and whether anything is depends on
    /// the piece.
    #[test]
    fn a_piece_ends_by_dying_away_rather_than_being_cut() {
        // Four moods rather than ten: a click is a property of the
        // scheduler's ending rather than of any recipe, and this is the
        // one test here that has to run at a real sample rate -- which
        // costs twenty times what the note-counting ones do. These four
        // are the ones whose pieces had something sounding at the end.
        for mood in [Mood::Night, Mood::Cave, Mood::Storm, Mood::Peril] {
            let rendered = Composer::render(mood, 240.0, 22_050, 3);
            let level = level_before_silence(&rendered, 22_050.0);
            assert!(
                level < 0.08,
                "{mood:?} was still at {:.0}% of its peak when it stopped",
                100.0 * level
            );
        }
    }

    /// **The silence between pieces has to be silence.** A rest that
    /// still has a tone in it is not a rest, and the whole argument for
    /// the rests -- that the game's own sounds are the thing being
    /// listened to -- depends on them being empty.
    #[test]
    fn the_silence_between_pieces_is_actually_silent() {
        let rate = 22_050.0;
        let mut composer = Composer::new(rate, 4);
        composer.wanted = Mood::Night;
        composer.start_piece();
        // Up to the end of the first piece, however long it turned out
        // to be.
        let mut frames = 0;
        while composer.phase != Phase::Resting && frames < (rate as usize * 200) {
            composer.next();
            frames += 1;
        }
        assert_eq!(composer.phase, Phase::Resting, "the piece never ended");

        // The release is under a second; a couple of seconds later
        // there must be nothing at all, and it must stay nothing.
        for _ in 0..(rate as usize * 3) {
            composer.next();
        }
        let mut loudest = 0.0f32;
        for _ in 0..(rate as usize * 10) {
            let (left, right) = composer.next();
            loudest = loudest.max(left.abs()).max(right.abs());
        }
        assert!(loudest < 1e-4, "the rest is still sounding at {loudest:.5}");
    }

    /// Changing mood mid-piece must fade rather than cut, and must
    /// actually arrive at the new mood.
    #[test]
    fn a_mood_change_is_answered() {
        let mut composer = Composer::new(RATE, 9);
        composer.wanted = Mood::Day;
        composer.start_piece();
        for _ in 0..(RATE as usize) {
            composer.next();
        }
        composer.set_mood(Mood::Cave);
        assert_eq!(composer.phase, Phase::Fading);
        // Long enough for the fade, the rest it schedules and the start
        // of the next piece.
        for _ in 0..(RATE as usize * 120) {
            composer.next();
            if composer.phase == Phase::Playing {
                break;
            }
        }
        assert_eq!(composer.playing, Mood::Cave);
    }
}

