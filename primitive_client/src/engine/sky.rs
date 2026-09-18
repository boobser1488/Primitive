//! Sky, sun and fog colour, derived from the server's world clock.
//!
//! The time of day is *server state*, not a local animation: the server
//! sends `TimeSync` and the client interpolates between those messages at
//! the day length it was told in `Welcome`. Two players standing next to
//! each other therefore see the same sunset at the same moment, which is
//! the whole point of syncing it rather than each client running its own
//! clock.
//!
//! Only the sun *direction* and *strength* change here. The per-block
//! light levels baked into the chunk meshes never move, so a full
//! day/night cycle costs zero re-meshing -- the shader does
//! `max(skylight × daylight, blocklight)` per fragment.

use glam::Vec3;

/// How much light there is with the sun below the horizon.
///
/// **This used to be 0.09 and read "so a moonlit night is navigable
/// rather than pitch black".** That was a defensible decision and it is
/// not the one this game makes any more: at 0.09 a midnight meadow came
/// out at a mean of 28 out of 255 -- dim daylight, not night. Every
/// shape in the world was still legible, so nothing about the dark was
/// a reason to do anything: no reason to carry fire, no reason to stop
/// walking, no reason for a camp to be anywhere in particular.
///
/// A quarter of that is what makes night a *thing that happens to you*
/// rather than a colour grade. What survives at 0.03 is the silhouette
/// against the sky and the ground immediately underfoot; what does not
/// is the far side of a field. That is the shape of a real night, and
/// it is the shape that makes a campfire worth the wood.
///
/// **Navigable is still true, and it is now true for a reason instead
/// of by subsidy.** Fire is early, cheap and already in the game; a
/// player who wants to travel at night lights the way, and one who does
/// not waits. Anybody who would rather not play that game raises
/// `ambient_light`, which is a setting and exists precisely for this
/// -- see `ClientSettings::ambient_light`, which was lowered with it.
///
/// **And this number is no longer the whole of how dark midnight is.**
/// Measured on an open meadow at midnight with the player's own
/// settings (`how_dark_the_night_is`): the ground came out at 13.5
/// levels of 255, of which the moonlight below was worth 4.8 and
/// `ambient_light` was worth 7.1. The floor was outshining the moon
/// nearly two to one, and lowering this constant could therefore only
/// ever reach a third of what a player could see -- which is why
/// "darker" had to be asked for twice. The shader takes the *larger* of
/// the two now rather than adding them (see `shade`), so at midnight
/// the world sits on the ambient floor and this number decides the
/// twilight that leads down to it.
const NIGHT_INTENSITY: f32 = 0.03;

/// What the moon makes of the night, as multiples: of [`NIGHT_INTENSITY`]
/// (`MOONLESS` and `FULL_MOON`) and of the ambient floor under open sky
/// (`MOONLESS_FLOOR` and `FULL_MOON_FLOOR`), with no moon up and with a full
/// moon high. See `primitive_shared::moon` for the phases.
///
/// **The floor is the half that shows.** At midnight the world sits on the
/// ambient floor (see the note on `NIGHT_INTENSITY`), so a moon that only
/// scaled the twilight figure would change nothing a player could see after
/// dusk. Under a roof or in a cave the floor is untouched -- the scale is
/// taken in proportion to the sky a face can see (`shade_lit_sky`) -- so a
/// moonless night is dark in the open and a cave is exactly as dark as it was.
///
/// The numbers make the full moon a night to walk through and the new moon
/// one to stay by the fire: the old single night sits between them, a little
/// nearer the dark end, because the old night is what "the dark is a thing
/// that happens to you" was written for.
const MOONLESS: f32 = 0.35;
const FULL_MOON: f32 = 1.8;
const MOONLESS_FLOOR: f32 = 0.45;
const FULL_MOON_FLOOR: f32 = 1.35;

/// How bright it is at the moment the sun touches the horizon.
///
/// **The two halves of the day curve meet here, and until now they did
/// not.** The figure below the horizon was written to fade from this
/// down to the night floor, and the figure above it was written to
/// climb from the *night floor* up to noon -- so at the instant the sun
/// reached zero the light dropped to 0.09, and the next instant it was
/// back at 0.35. A dive into darkness and a jump out of it, both inside
/// a fifth of a second at the day length this game runs.
///
/// Measured over the whole day at a thousandth of it per sample, the
/// largest single step was **0.2472** and it sat at 0.751 -- sunset,
/// exactly. Joined, the largest step anywhere is **0.0310**, and it is
/// at dawn where the sun is climbing fastest. Noon and midnight are
/// unchanged, and so is every value after the sun is down: the night
/// branch was right, and it is the day branch that now ends where the
/// night branch begins.
///
/// That the sun on the horizon leaves a third of the light is not a
/// concession to the arithmetic. It is civil twilight, and a world that
/// went night-dark the moment the disc touched the hills would be
/// wrong in the other direction.
const DUSK_INTENSITY: f32 = 0.35;

/// How steeply the light falls once the sun is under the horizon: the power
/// the sun's depth is raised to between [`DUSK_INTENSITY`] and the night.
///
/// **"Ночь быстро наступает".** It was eight, which is close to the real
/// thing -- on Earth the light is all but gone by the time the sun is
/// eighteen degrees down, and eight puts this curve there too. The trouble is
/// that a day here is not a day: twenty minutes against twenty-four hours,
/// seventy-two times faster, and a physically right twilight inside it lasts
/// **forty-five seconds**. A player who turns round at sunset finds it night.
///
/// Three ways to give dusk back were weighed:
///
/// * *Lengthen the day alone.* Real dusk is a thirtieth of a day, so a day
///   long enough for a two-minute dusk is an hour long, and the night in it
///   half an hour.
/// * *Hold the light and then drop it.* A plateau followed by a fall is the
///   hole this curve was written to close -- see [`DUSK_INTENSITY`].
/// * **A gentler power (chosen).** The shape stays a fall from dusk to the
///   night floor, with midnight and noon untouched; only its middle is
///   stretched. At four, dusk takes a hundred seconds of a twenty-minute day
///   -- the sky has time to go through its colours and a player has time to
///   get home. It is the day that is compressed, so the twilight inside it is
///   stretched to match; `dusk_takes_minutes_rather_than_three_quarters_of_one`
///   is what holds it there.
const TWILIGHT_FALL: i32 = 4;

/// The clear sky with the sun well up, at every lighting step. The fog
/// under water measures the hour against it (`fog::Fog::color`).
pub(crate) const DAY_SKY: Vec3 = Vec3::new(0.53, 0.80, 0.92);
const SUNSET_SKY: Vec3 = Vec3::new(0.85, 0.45, 0.28);
const NIGHT_SKY: Vec3 = Vec3::new(0.02, 0.03, 0.08);

/// What the moon makes of the night sky's colour, as multiples of
/// [`NIGHT_SKY`]: with no moon up, and with a full moon high.
///
/// **The sky was the one part of the night the moon did not reach.** The
/// first pictures of the phases (`what_the_moon_looks_like`) had the ground
/// under the new moon plainly darker than under the full, and a frame only
/// thirteen per cent darker, because the half of it that was sky was the same
/// blue on both nights. A moonless sky is the darker one, and the fog is
/// drawn from this colour, so the horizon goes with it.
const MOONLESS_SKY: f32 = 0.6;
const FULL_MOON_SKY: f32 = 1.3;

/// How fast the sky catches up with the weather, in units of `overcast`
/// per second.
///
/// A twelfth, so a clear sky takes about twelve seconds to become a full
/// storm and as long again to clear. Long enough to be a front arriving
/// rather than a switch being thrown, short enough that a player who
/// typed `/weather storm` sees it happen rather than wondering whether
/// the command worked.
const OVERCAST_RATE: f32 = 1.0 / 12.0;

pub struct Sky {
    /// The world's age in days at the last sync, hour in the fraction.
    /// The season is read off this plus whatever the hour has done since
    /// -- see `world_days`.
    days_at_sync: f32,
    /// 0.0 = midnight, 0.5 = noon.
    pub time_of_day: f32,
    day_length_seconds: f32,
    /// Where the server last told us the clock was; we ease toward it
    /// rather than snapping, so a late `TimeSync` doesn't visibly jolt
    /// the sun.
    target_time: f32,
    /// What is falling out of it. See `set_weather`.
    weather: primitive_shared::weather::Weather,
    /// How far the sky has *actually* got toward that weather, 0..1.
    ///
    /// Weather arrives over a socket as a step -- one message, and it
    /// was clear and now it is a storm. A sky that took the step
    /// literally would go from blue to slate between two frames, which
    /// no weather has ever done and which reads as the renderer
    /// glitching rather than as a front coming in. So the *message* is
    /// instant and the *sky* is not: this eases toward
    /// `weather.intensity()` at `OVERCAST_RATE`, and everything the
    /// weather touches -- the light, the sky colour, the cloud deck --
    /// reads this rather than the enum.
    overcast: f32,
    /// Seconds since this sky was made.
    ///
    /// The cloud layer is the only thing that reads it, and it has to:
    /// `time_of_day` wraps at midnight, and a drift driven by a number
    /// that wraps snaps the whole sky back to where it was a day ago.
    /// Clouds are also the one part of the sky nobody expects two
    /// players to agree about, so a purely local clock costs nothing.
    elapsed: f32,
    /// How far the wind has carried the cloud deck, in the units the sky
    /// shader samples the layer in (`CLOUD_SCALE` a block), wrapped at
    /// `CLOUD_WRAP`. See `blow_the_clouds`.
    cloud_drift: glam::Vec2,
}

/// How many of the cloud layer's sampling units one block is -- the
/// `0.0022` in `sky.wgsl` where the view ray meets the deck. The two have to
/// agree or the deck moves at a speed that is not the wind's.
const CLOUD_SCALE: f32 = 0.0022;

/// Where the drift wraps, in those units, so a session left running for a
/// week does not sample the picture at a coordinate a float cannot hold.
///
/// **Thirteen tiles, and not one.** The wrap has to land the deck exactly on
/// itself: a whole number of tiles of the picture (`CLOUD_TILE`, six, in
/// `sky.wgsl`) *and* a whole number of the grid's cells (`CLOUD_PIXEL`,
/// 0.026) -- one tile is 230.8 cells, so wrapping at six jumped every cloud a
/// fraction of a cell sideways every few minutes of storm. Seventy-eight is
/// three thousand cells.
const CLOUD_WRAP: f32 = 78.0;

/// How fast the deck runs, in blocks a second: a floor that even a dead
/// calm keeps (a sky whose clouds stood still would read as a painting), and
/// the wind's own strength on top.
///
/// **The weather's clouds run with the weather's wind** (`raft::wind`), and
/// it was a fixed slant on a local clock -- the same slow drift toward the
/// same corner whatever the sail below it was braced against. A storm deck
/// crossing the sky at four times the pace of a fair one is most of what a
/// squall looks like before any rain has fallen, and the direction is the
/// one the rain slants in and the raft is pushed, so the three agree.
/// Twelve at full strength is a real gale's cloud at the height the deck is
/// drawn (a block is a metre): fast enough to see move in a glance, not so
/// fast the pixels of the grid flicker across a cell a frame.
const CLOUD_CALM_SPEED: f32 = 1.2;
const CLOUD_WIND_SPEED: f32 = 12.0;

impl Sky {
    pub fn new(time_of_day: f32, day_length_seconds: f32) -> Self {
        Self {
            days_at_sync: time_of_day.rem_euclid(1.0),
            time_of_day: time_of_day.rem_euclid(1.0),
            day_length_seconds: day_length_seconds.max(1.0),
            target_time: time_of_day.rem_euclid(1.0),
            // A world opens dry, and so does a session: the server
            // sends the real weather with the handshake, and starting
            // from clear means the first frame is never a storm that
            // clears a moment later.
            weather: primitive_shared::weather::Weather::Clear,
            overcast: 0.0,
            elapsed: 0.0,
            cloud_drift: glam::Vec2::ZERO,
        }
    }

    /// Seconds since this sky was made -- the cloud drift's clock.
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// What the server says the time is.
    ///
    /// **Glided, but only when the correction is small enough to be
    /// drift.** The client runs the same clock at the same rate, so an
    /// ordinary sync is a few thousandths out and easing onto it hides
    /// the step. A *large* delta is not drift -- it is a session that
    /// has just begun, or a client that was asleep in a paused window --
    /// and easing onto that is the sun visibly sliding across the sky
    /// for a second and a half, which is the thing the player reported.
    /// A jump nobody can see is better than a slide everybody can.
    pub fn on_time_sync(&mut self, time_of_day: f32, world_days: f32) {
        self.days_at_sync = world_days;
        let wanted = time_of_day.rem_euclid(1.0);
        self.target_time = wanted;
        if shortest_way_round(wanted - self.time_of_day).abs() > SNAP_BEYOND {
            self.time_of_day = wanted;
        }
    }

    /// The world's age in days with the hour in the fraction: the last
    /// sync's day count carried forward by the hour the sky has kept
    /// since. Syncs come every couple of seconds, so the only thing to
    /// get right is midnight: if the hour has wrapped since the sync,
    /// a day has passed.
    pub fn world_days(&self) -> f32 {
        let day = self.days_at_sync.floor();
        let hour_at_sync = self.days_at_sync - day;
        let wrapped = self.time_of_day + 0.5 < hour_at_sync;
        day + self.time_of_day + if wrapped { 1.0 } else { 0.0 }
    }

    pub fn tick(&mut self, dt: f32) {
        self.elapsed += dt.max(0.0);
        self.time_of_day = (self.time_of_day + dt / self.day_length_seconds).rem_euclid(1.0);

        let delta = shortest_way_round(self.target_time - self.time_of_day);
        self.time_of_day = (self.time_of_day + delta * (dt * 2.0).min(1.0)).rem_euclid(1.0);
        self.target_time = (self.target_time + dt / self.day_length_seconds).rem_euclid(1.0);
        self.step_the_weather(dt);
        self.blow_the_clouds(dt);
    }

    /// Moves the deck on with the world's wind.
    ///
    /// Integrated rather than worked out from the clock, because the wind
    /// veers: a position that was `wind * time` would swing the whole sky
    /// round the player every time the direction changed, where a deck that
    /// has been *carried* just starts going the new way from where it is.
    ///
    /// The sign is the shader's: the layer is sampled at `world + drift`, so
    /// for the clouds to travel *toward* the wind the drift runs against it.
    fn blow_the_clouds(&mut self, dt: f32) {
        let wind = primitive_shared::raft::wind(self.world_days(), self.weather);
        let (sin, cos) = wind.toward.sin_cos();
        let speed = CLOUD_CALM_SPEED + CLOUD_WIND_SPEED * wind.strength;
        let step = glam::Vec2::new(cos, sin) * speed * CLOUD_SCALE * dt.max(0.0);
        self.cloud_drift = (self.cloud_drift - step).rem_euclid(glam::Vec2::splat(CLOUD_WRAP));
    }

    /// How far the wind has carried the deck, for the sky shader. See
    /// `blow_the_clouds`.
    pub fn cloud_drift(&self) -> glam::Vec2 {
        self.cloud_drift
    }

    /// How much of the weather's rain has reached the ground yet, 0..1.
    ///
    /// **The cloud comes first and the rain after it.** The message is a
    /// step and the deck eases in over a quarter of a minute (`overcast`),
    /// but the drops used to start on the frame the message came -- a
    /// downpour out of a sky that was still blue, the clouds catching up
    /// with the rain rather than bringing it. Now nothing falls until the
    /// deck is half way to the weather's own cover, and the shower is at
    /// full strength only once the sky has closed: a player sees the light
    /// go and the clouds run dark and has a few seconds to act on it, which
    /// is what the sky over a real front gives.
    ///
    /// Rejected: a forecast from the server, so the deck could darken a
    /// minute ahead. It would be a message and a saved field for a warning,
    /// and the rain in this game is a five-minute spell -- the few seconds
    /// the eased deck gives are the warning at the scale the weather has.
    /// When the weather clears the rain stops at once and the deck lingers,
    /// which is the right way round as well.
    pub fn rain_arrived(&self) -> f32 {
        let wanted = self.weather.intensity();
        if wanted <= 0.0 {
            return 0.0;
        }
        let t = ((self.overcast / wanted - 0.5) / 0.4).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    fn step_the_weather(&mut self, dt: f32) {
        // The front rolls in. A step toward the weather rather than at
        // it, so the whole sky -- light, colour and cloud deck -- moves
        // together and takes about a quarter of a minute to do it.
        let wanted = self.weather.intensity();
        let step = OVERCAST_RATE * dt.max(0.0);
        self.overcast = if self.overcast < wanted {
            (self.overcast + step).min(wanted)
        } else {
            (self.overcast - step).max(wanted)
        };
    }

    /// How thoroughly the sky is covered, 0..1 -- the eased figure, not
    /// the enum's. What the cloud deck is drawn from and what the light
    /// is dimmed by.
    pub fn overcast(&self) -> f32 {
        self.overcast
    }

    /// Height of the sun above the horizon, -1..1. 0 = exactly at the
    /// horizon, 1 = directly overhead.
    pub fn sun_elevation(&self) -> f32 {
        ((self.time_of_day - 0.25) * std::f32::consts::TAU).sin()
    }

    /// Direction the sunlight *travels* (from the sun toward the ground),
    /// which is what the shader wants for a dot product against a face
    /// normal.
    pub fn sun_direction(&self) -> Vec3 {
        let angle = (self.time_of_day - 0.25) * std::f32::consts::TAU;
        // A little Z tilt so faces on the north/south axis aren't lit
        // perfectly evenly all day, which reads as flat.
        let to_sun = Vec3::new(angle.cos(), angle.sin(), 0.25).normalize();
        -to_sun
    }

    /// Direction the moonlight travels, as [`sun_direction`](Self::sun_direction)
    /// is for the sun.
    ///
    /// **Placed by the phase.** The moon rides the sun's circle, behind the
    /// sun by the share of a turn its phase is (`moon::phase`): beside the sun
    /// at the new moon, and so up by day and not at night; opposite it at the
    /// full moon, up all night; high in the evening at first quarter and in
    /// the morning at the last. The sky draws the lit part from where the sun
    /// is (`sky.wgsl`), so this is also what gives a crescent its shape.
    pub fn moon_direction(&self) -> Vec3 {
        let angle = (self.time_of_day - 0.25) * std::f32::consts::TAU
            - primitive_shared::moon::phase(self.world_days()) * std::f32::consts::TAU;
        -Vec3::new(angle.cos(), angle.sin(), 0.25).normalize()
    }

    /// How much moonlight falls on open ground, 0..1: how much of the moon is
    /// lit, times how high it stands. A full moon still under the horizon
    /// lights nothing, which is what makes the evening before a full moon
    /// dark until it rises.
    pub fn moonlight(&self) -> f32 {
        let height = -self.moon_direction().y;
        primitive_shared::moon::illumination(self.world_days()) * smoothstep(0.0, 0.25, height)
    }

    /// What the ambient floor under open sky is scaled by: one by day, and at
    /// night from `MOONLESS_FLOOR` to `FULL_MOON_FLOOR` by the moonlight.
    /// Eased in over the last of the dusk so the floor does not step when the
    /// sun goes down.
    pub fn night_floor(&self) -> f32 {
        let moon = MOONLESS_FLOOR + (FULL_MOON_FLOOR - MOONLESS_FLOOR) * self.moonlight();
        let day = smoothstep(-0.2, 0.0, self.sun_elevation());
        moon + (1.0 - moon) * day
    }

    /// The moon as the shaders read it (`Globals::moon`): the direction its
    /// light travels, and the night floor's scale less one -- so a tool that
    /// fills its globals with zeroes draws the night as it always was.
    pub fn moon_uniform(&self) -> [f32; 4] {
        let direction = self.moon_direction();
        [direction.x, direction.y, direction.z, self.night_floor() - 1.0]
    }

    /// How strongly skylight is scaled right now.
    /// What the sky is doing, as the server last said.
    ///
    /// A field here rather than a parameter on everything downstream,
    /// because *everything* downstream wants it: the sun that lights the
    /// blocks, the colour behind the terrain, and the fog that is drawn
    /// from that colour. One multiply in `sun_intensity` and one blend
    /// in `sky_color` makes a storm dark all the way through, and the
    /// alternative is the same two changes made in four files that each
    /// hold a quarter of the answer -- which is the arrangement `fog`
    /// was written to end.
    pub fn set_weather(&mut self, weather: primitive_shared::weather::Weather) {
        self.weather = weather;
    }

    /// What it is doing, for anything that wants to ask rather than be
    /// told. Nothing does today; the field is written by the frame loop
    /// and read by the two functions below it.
    #[allow(dead_code)]
    pub fn weather(&self) -> primitive_shared::weather::Weather {
        self.weather
    }

    /// How strongly the world is lit right now, weather included.
    ///
    /// **The weather multiply was missing.** The note on `set_weather`
    /// has always said a storm is dark all the way through -- one
    /// multiply here, one blend in `sky_color` -- and only the second
    /// half of it was ever written. So a storm greyed the sky and left
    /// the ground lit like noon, which is the one arrangement that
    /// reads as a bug rather than as weather: the light says midday and
    /// the sky says otherwise. It is here now, on the eased figure, so
    /// the world darkens as the front arrives rather than at the moment
    /// the packet lands.
    pub fn sun_intensity(&self) -> f32 {
        self.clear_sun_intensity() * self.daylight_factor()
    }

    /// What the weather leaves of the daylight, from the eased overcast
    /// rather than from the enum.
    ///
    /// `Weather::daylight_factor` is the same curve at the two ends and
    /// cannot be used directly, because it takes the enum -- which
    /// steps. Both go through `weather::daylight_under`, so the sky and
    /// the light cannot come to disagree about how dark a storm is.
    fn daylight_factor(&self) -> f32 {
        primitive_shared::weather::daylight_under(self.overcast)
    }

    /// The sun with the weather taken off again: what it would be doing
    /// over a clear sky.
    ///
    /// The sky colour needs this one rather than the public figure, or
    /// the storm grey would be dimmed by the storm twice over and a
    /// rainy noon would come out darker than the night it is blended
    /// against.
    fn clear_sun_intensity(&self) -> f32 {
        let e = self.sun_elevation();
        if e <= 0.0 {
            // Below the horizon: fade from dusk to the night floor over
            // the last of the light rather than cutting to black. The night
            // is the moon's: see `MOONLESS` and `FULL_MOON`.
            let night = NIGHT_INTENSITY * (MOONLESS + (FULL_MOON - MOONLESS) * self.moonlight());
            night + (1.0 + e).max(0.0).powi(TWILIGHT_FALL) * (DUSK_INTENSITY - night)
        } else {
            // ...and above it, climb from that same dusk figure to
            // noon. Reading the two from one constant is the whole fix:
            // written separately they disagreed, and the disagreement
            // was a hole in the light at the exact moment a player
            // watches the sky. See `DUSK_INTENSITY`.
            (DUSK_INTENSITY + e.powf(0.6) * (1.0 - DUSK_INTENSITY)).min(1.0)
        }
    }

    /// What colour the **direct** light is, at luminance one.
    ///
    /// **Hue only. How bright it is stays [`sun_intensity`]'s job**, and
    /// keeping the two apart is the whole design: a sunset is not merely
    /// a dim noon, and a dim noon is not orange. Normalising to
    /// luminance one means adding this to the shading model changed the
    /// exposure of the game by nothing at all -- only what colour the
    /// light arriving at a surface is.
    ///
    /// That matters more than it sounds. Multiplying an albedo by a
    /// *scalar* keeps its hue and its saturation identical at every
    /// brightness, which is the definition of a material that does not
    /// respond to its light -- and it is exactly what makes a rendered
    /// world look moulded out of plastic. A lit face and a shaded face
    /// of the same block should not be the same colour twice at
    /// different volumes.
    ///
    /// Three regimes, because there are three lights in a day:
    ///
    /// * **Overhead** -- barely warm. Direct sunlight through little
    ///   atmosphere is close to white, and pushing it further is what
    ///   makes a midday screenshot look like a filter.
    /// * **At the horizon** -- strongly orange. The light is coming
    ///   through a great deal more air, which is a physical fact and
    ///   also the best-looking half hour in any game that has one.
    /// * **Below it** -- cool. Moonlight is sunlight off a grey rock,
    ///   and the blue is the eye's, not the moon's; either way it is
    ///   what night looks like and what stops the small hours reading as
    ///   underexposed afternoon.
    pub fn sun_color(&self) -> Vec3 {
        const NOON: Vec3 = Vec3::new(1.00, 0.97, 0.91);
        const HORIZON: Vec3 = Vec3::new(1.00, 0.62, 0.34);
        const MOON: Vec3 = Vec3::new(0.72, 0.80, 1.00);

        let e = self.sun_elevation();
        let colour = if e >= 0.0 {
            // Fast out of the orange: the sun spends very little of the
            // day near the horizon, and a linear ramp leaves the whole
            // morning looking like dawn.
            HORIZON.lerp(NOON, smoothstep(0.0, 0.22, e))
        } else {
            HORIZON.lerp(MOON, smoothstep(0.0, -0.12, e))
        };

        // ...and the weather takes the colour out of it as well as the
        // brightness. Overcast light is the sky's own diffuse white,
        // because there is no direct beam left to be warm.
        normalised(colour.lerp(Vec3::splat(1.0), 1.0 - self.daylight_factor()))
    }

    /// What colour the **fill** light is: the sky itself, at luminance
    /// one.
    ///
    /// The other half of the same idea. Outdoors, the light reaching a
    /// face the sun cannot see comes from the sky, and the sky is blue
    /// -- so a shadow is not a darker version of the lit surface, it is
    /// a *cooler* one. That single relation is most of what the eye uses
    /// to read a surface as real, and its absence is most of what reads
    /// as plastic.
    ///
    /// Taken from [`sky_color`] rather than invented, so the light in
    /// the world and the sky over it cannot disagree -- including when
    /// the weather turns the sky grey, at which point the fill goes grey
    /// with it on its own.
    pub fn fill_color(&self) -> Vec3 {
        normalised(self.sky_color())
    }

    pub fn sky_color(&self) -> Vec3 {
        // Toward a flat storm grey rather than simply darker: a rain
        // cloud is not a dim blue sky, and darkening the blue is what
        // makes weather in most games read as dusk arriving early.
        const OVERCAST: Vec3 = Vec3::new(0.42, 0.44, 0.48);
        let clear = self.clear_sky_color();
        clear.lerp(
            OVERCAST * self.clear_sun_intensity().max(0.25),
            1.0 - self.daylight_factor(),
        )
    }

    /// The sky the weather is drawn over: what it would look like with
    /// nothing falling.
    fn clear_sky_color(&self) -> Vec3 {
        let e = self.sun_elevation();
        let night_sky = self.night_sky();
        if e >= 0.25 {
            DAY_SKY
        } else if e >= 0.0 {
            SUNSET_SKY.lerp(DAY_SKY, smoothstep(0.0, 0.25, e))
        } else if e >= -0.2 {
            night_sky.lerp(SUNSET_SKY, smoothstep(-0.2, 0.0, e))
        } else {
            night_sky
        }
    }

    /// The deep night's sky colour under tonight's moon. See `MOONLESS_SKY`.
    fn night_sky(&self) -> Vec3 {
        NIGHT_SKY * (MOONLESS_SKY + (FULL_MOON_SKY - MOONLESS_SKY) * self.moonlight())
    }

    /// [`sun_color`](Self::sun_color), for a lighting step.
    ///
    /// **Golden for most of the morning, not only at the horizon.** The
    /// Simple ramp leaves the orange by the time the sun is thirteen
    /// degrees up and is near-white from there on, which is right for a
    /// light that is the whole of what a face receives. At Balanced it is
    /// not: the shader hands the part of the light below the half-Lambert
    /// floor to the sky's cool fill (see `shade_lit`), and a warm key laid
    /// over a cool fill mixes back toward neutral on a lit face. So the
    /// key carries more warmth for longer -- amber to about six degrees,
    /// honey to about eighteen, and a noon that is still a shade to the
    /// yellow side of white -- and what reaches a sunlit face is the
    /// warmth that was asked for rather than the plain noon it would have
    /// cancelled back to. `the_noon_sun_is_warm_and_not_a_filter_at_every_step`
    /// holds the noon end honest.
    ///
    /// **The first ramp was too timid, and the pictures said so.** It was
    /// gold to six degrees and honey to eighteen; photographed over the
    /// savanna (`what_the_lighting_looks_like`), golden hour at eighteen
    /// degrees came out indistinguishable from Simple, and the whole
    /// frame at dawn was *less* warm than Simple's (red less blue 25
    /// against 46, at the same brightness) -- the cooler shade had eaten
    /// the warmth and nothing had put it back. So gold now lasts to ten
    /// degrees, honey to twenty-seven, and noon sits as far to the yellow
    /// side of white as the test lets it.
    pub fn sun_color_for(&self, quality: crate::engine::lighting::Quality) -> Vec3 {
        if quality.is_simple() {
            return self.sun_color();
        }
        const EMBER: Vec3 = Vec3::new(1.00, 0.50, 0.22);
        const GOLD: Vec3 = Vec3::new(1.00, 0.70, 0.36);
        const HONEY: Vec3 = Vec3::new(1.00, 0.83, 0.56);
        const NOON: Vec3 = Vec3::new(1.00, 0.92, 0.78);
        const MOON: Vec3 = Vec3::new(0.70, 0.80, 1.00);

        let e = self.sun_elevation();
        let colour = if e >= 0.45 {
            HONEY.lerp(NOON, smoothstep(0.45, 0.80, e))
        } else if e >= 0.18 {
            GOLD.lerp(HONEY, smoothstep(0.18, 0.45, e))
        } else if e >= 0.0 {
            EMBER.lerp(GOLD, smoothstep(0.0, 0.18, e))
        } else {
            EMBER.lerp(MOON, smoothstep(0.0, -0.12, e))
        };
        normalised(colour.lerp(Vec3::splat(1.0), 1.0 - self.daylight_factor()))
    }

    /// [`fill_color`](Self::fill_color), for a lighting step.
    ///
    /// **The sky's own colour, with some of its saturation taken out.**
    /// At Balanced the fill carries nearly half of what a sunlit top
    /// receives and all of what a face in shadow does, and the raw sky
    /// normalised to luminance one is `(0.71, 1.07, 1.23)` at noon: laid
    /// over grey stone at that share it turned a shaded wall teal, which
    /// is a filter rather than shade. Seven tenths of the way toward the
    /// sky was the first try, and it was still too much: a canopy at dawn
    /// lost the gold Simple gave it and came out a neutral green. Just
    /// over half is plainly cooler than the key, which is the relation the
    /// eye reads, and leaves the world its own colours.
    ///
    /// **And under a low sun the shade takes on the sunset.** The sky that
    /// lights a face turned away from the sun at dusk is not the lavender
    /// overhead; a great deal of it is the glowing band on the horizon.
    /// Leaving that out made every shaded face at sunset violet, which is
    /// the one moment warm shade is right. So the fill leans toward the
    /// glow's colour by as much as the glow is there -- nothing at noon,
    /// four tenths of the way at its peak.
    pub fn fill_color_for(&self, quality: crate::engine::lighting::Quality) -> Vec3 {
        if quality.is_simple() {
            return self.fill_color();
        }
        const SATURATION: f32 = 0.55;
        const SUNSET_SHARE: f32 = 0.4;
        let sky = Vec3::ONE.lerp(normalised(self.sky_color_for(quality)), SATURATION);
        let glow = self.horizon_glow(quality);
        normalised(sky.lerp(normalised(glow.truncate()), glow.w * SUNSET_SHARE))
    }

    /// [`sky_color`](Self::sky_color), for a lighting step: the one colour
    /// of the sky overhead, which the fog is drawn from.
    ///
    /// **Noon is the same flat blue at every step**, because that is what
    /// the player asked the sky to be. What changes is the evening. Simple
    /// turns the *whole* sky orange while the sun sets, and since the fog
    /// is this colour, so is every distant hill in every direction -- the
    /// world looks as if it were seen through amber glass, and the side
    /// away from the sun is as orange as the side toward it. Past Simple
    /// the sky away from the sun goes to a dusky lavender and then a deep
    /// blue-violet twilight, and the orange is put back only where it
    /// belongs, around the sun, by [`horizon_glow`](Self::horizon_glow).
    pub fn sky_color_for(&self, quality: crate::engine::lighting::Quality) -> Vec3 {
        if quality.is_simple() {
            return self.sky_color();
        }
        // Both a shade less violet than the first pass, which painted
        // dusk over the savanna a saturated purple -- pretty in one
        // picture and a filter in the next thousand.
        const DUSK: Vec3 = Vec3::new(0.60, 0.56, 0.68);
        const TWILIGHT: Vec3 = Vec3::new(0.20, 0.19, 0.33);
        const OVERCAST: Vec3 = Vec3::new(0.42, 0.44, 0.48);

        let e = self.sun_elevation();
        let night_sky = self.night_sky();
        let clear = if e >= 0.25 {
            DAY_SKY
        } else if e >= 0.0 {
            DUSK.lerp(DAY_SKY, smoothstep(0.0, 0.25, e))
        } else if e >= -0.10 {
            TWILIGHT.lerp(DUSK, smoothstep(-0.10, 0.0, e))
        } else if e >= -0.22 {
            night_sky.lerp(TWILIGHT, smoothstep(-0.22, -0.10, e))
        } else {
            night_sky
        };
        clear.lerp(
            OVERCAST * self.clear_sun_intensity().max(0.25),
            1.0 - self.daylight_factor(),
        )
    }

    /// The glow on the horizon under a low sun: its colour in `xyz`, how
    /// strongly it replaces the sky's own colour in `w`, 0..1.
    ///
    /// **A mix toward a colour, not light added on top.** Added, a strong
    /// orange over a lavender sky clips its red and lands on salmon pink;
    /// mixed, the horizon under the sun is the colour named here and the
    /// sky beside it is the colour it was.
    ///
    /// Where it is drawn is the shaders' business (`horizon_glow` in both
    /// `shader.wgsl` and `sky.wgsl`, which must agree or the edge of the
    /// world shows as a line): a lobe around the sun's compass bearing,
    /// fading upward from the horizon. This is only *how much* and *what
    /// colour*, and both follow the elevation:
    ///
    /// * nothing above about twenty-five degrees -- the flat noon sky is
    ///   untouched, as the player asked for it;
    /// * a thin honey band through the morning and afternoon, amber from
    ///   six degrees down, flame at the horizon, and crimson then a
    ///   dusky rose once the sun is under it -- the colour a real sky
    ///   keeps in the west for half an hour after sunset;
    /// * gone by about thirteen degrees below, which is where the sky
    ///   itself has gone to night.
    ///
    /// Weather takes it away twice over (the square of what is left of
    /// the sky), because a sunset behind a closing deck does not fade in
    /// step with the light -- it is the first thing to go.
    pub fn horizon_glow(&self, quality: crate::engine::lighting::Quality) -> glam::Vec4 {
        if quality.is_simple() {
            return glam::Vec4::ZERO;
        }
        const HONEY: Vec3 = Vec3::new(1.00, 0.80, 0.52);
        const AMBER: Vec3 = Vec3::new(1.00, 0.62, 0.32);
        const FLAME: Vec3 = Vec3::new(1.00, 0.47, 0.22);
        const CRIMSON: Vec3 = Vec3::new(0.78, 0.30, 0.26);
        const ROSE: Vec3 = Vec3::new(0.42, 0.18, 0.30);
        const PEAK: f32 = 0.85;

        let e = self.sun_elevation();
        let colour = if e >= 0.25 {
            HONEY
        } else if e >= 0.08 {
            AMBER.lerp(HONEY, smoothstep(0.08, 0.25, e))
        } else if e >= 0.0 {
            FLAME.lerp(AMBER, smoothstep(0.0, 0.08, e))
        } else if e >= -0.10 {
            CRIMSON.lerp(FLAME, smoothstep(-0.10, 0.0, e))
        } else {
            ROSE.lerp(CRIMSON, smoothstep(-0.22, -0.10, e))
        };
        let rising = smoothstep(0.42, 0.02, e);
        let setting = smoothstep(-0.22, -0.02, e);
        let clear = 1.0 - self.overcast.clamp(0.0, 1.0);
        colour.extend(PEAK * rising * setting * clear * clear)
    }

    /// Light scattered around the sun, added to the sky and to the
    /// distance in its direction: a colour in `xyz`, already scaled by its
    /// strength, and `w` unused. Only at High; zero otherwise.
    ///
    /// Strongest and warmest low down, where the light crosses the most
    /// air and the haze toward the sun is what a real morning looks like;
    /// weak and nearly white overhead. Nothing at night -- the moon's
    /// light is a floor, not a source anything scatters -- and nothing
    /// under weather, which is the scattering taken to its end.
    pub fn sun_haze(&self, quality: crate::engine::lighting::Quality) -> glam::Vec4 {
        if quality != crate::engine::lighting::Quality::High {
            return glam::Vec4::ZERO;
        }
        const LOW: Vec3 = Vec3::new(1.00, 0.66, 0.36);
        const HIGH: Vec3 = Vec3::new(1.00, 0.95, 0.84);
        let e = self.sun_elevation();
        let up = smoothstep(-0.06, 0.04, e);
        let low_sun = 1.0 - smoothstep(0.05, 0.6, e);
        let strength = up * (0.14 + 0.22 * low_sun) * (1.0 - self.overcast.clamp(0.0, 1.0));
        (LOW.lerp(HIGH, 1.0 - low_sun) * strength).extend(0.0)
    }

    pub fn clock_string(&self) -> String {
        let minutes = (self.time_of_day * 24.0 * 60.0) as i32;
        format!("{:02}:{:02}", (minutes / 60) % 24, minutes % 60)
    }
}

/// A colour scaled to luminance one, so it carries hue and nothing else.
///
/// Rec. 709 weights: the eye is not equally sensitive to the three
/// channels, and dividing by a flat average would make a blue fill
/// genuinely darker than a white one rather than merely bluer.
fn normalised(colour: Vec3) -> Vec3 {
    const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);
    let luminance = colour.dot(LUMA);
    if luminance > 1e-4 {
        colour / luminance
    } else {
        Vec3::splat(1.0)
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where a time is relative to another, across the 0.0/1.0 wrap.
///
/// Midnight is a seam, not an edge: a target a hundredth *before* 0.00
/// is a hundredth away, not most of a day away. Written once because
/// both the sync and the glide ask it, and they have to agree -- a
/// correction judged large by one and small by the other is a sky that
/// snaps and then slides.
fn shortest_way_round(delta: f32) -> f32 {
    if delta > 0.5 {
        delta - 1.0
    } else if delta < -0.5 {
        delta + 1.0
    } else {
        delta
    }
}

/// How far out the client has to be before a sync is a jump rather than
/// a glide.
///
/// Two hundredths of a day, which at the fifteen-minute day this game
/// runs is eighteen seconds -- far more than a frame of drift and far
/// less than the difference between a menu at dusk and a world at noon.
const SNAP_BEYOND: f32 = 0.02;

#[cfg(test)]
mod tests {
    /// **The ambient floor is a floor, and the shader has to keep
    /// taking it as one.**
    ///
    /// `direct + fill + ambient` is what this line used to be, and it
    /// reads as obviously right: a little light everywhere, added to
    /// whatever else there is. By day the difference is invisible --
    /// noon is fifty times the floor. At midnight it was more than half
    /// of everything: measured on an open meadow with the player's own
    /// settings, the ground came out at 13.5 levels of 255, of which
    /// this term was 7.1 and the moon was 4.8. So *the night constant
    /// could not reach half of what the player could see*, and "make
    /// the night darker" was asked for twice and answered in the wrong
    /// place both times.
    ///
    /// Read as source rather than run on a GPU, in the same way and for
    /// the same reason as `the_solid_entry_point_does_not_discard`: the
    /// property is one expression in a shading language, and a test
    /// that needs an adapter is a test that does not run in CI.
    #[test]
    fn the_ambient_floor_is_taken_as_a_floor_and_not_added_on_top() {
        let source = include_str!("shader.wgsl");
        assert!(
            // `light_floor` is `AMBIENT_COLOR` at the Simple step and a
            // colour at the same luminance past it -- see `MOONLIT_FLOOR`.
            source.contains("max(direct + fill, globals.fog_params.z * light_floor)"),
            "the ambient is no longer taken as the floor it is documented to be",
        );
        assert!(
            !source.contains("direct + fill + globals.fog_params.z"),
            "the ambient is being added to the light again, which is what made midnight bright",
        );
    }

    /// A big correction is a jump; a small one is a glide.
    ///
    /// **Both halves matter and they pull opposite ways.** Easing hides
    /// the thousandths of drift between two clocks running the same day
    /// at the same rate, which is what a sync usually is. Easing a
    /// *large* delta -- a session that has just begun, a window that
    /// was paused for a minute -- is the sun visibly sliding across the
    /// sky, which is what the player reported as "заметно
    /// подстраивается под игровое время". A jump nobody can see beats a
    /// slide everybody can.
    /// The day count rides on the last sync and the hour on the sky's
    /// own clock; when the hour wraps past midnight between syncs, the
    /// day has to turn with it, or the season would jump back for two
    /// seconds every night.
    #[test]
    fn the_calendar_carries_the_day_across_midnight() {
        let mut sky = Sky::new(0.99, 100.0);
        sky.on_time_sync(0.99, 7.99);
        assert!((sky.world_days() - 7.99).abs() < 1e-5);
        // The sky's own hour crosses midnight before the next sync
        // arrives (`tick` would also drift it back towards the last
        // sync, which is why the hour is set outright here).
        sky.time_of_day = 0.01;
        assert!((sky.world_days() - 8.01).abs() < 1e-4, "day 8 should have begun: {}", sky.world_days());
        // An hour that merely runs a little behind the sync is the same day.
        sky.time_of_day = 0.98;
        assert!((sky.world_days() - 7.98).abs() < 1e-4, "still day 7: {}", sky.world_days());
        // ...and the sync that follows the wrap agrees with the estimate.
        sky.on_time_sync(0.02, 8.02);
        sky.time_of_day = 0.02;
        assert!((sky.world_days() - 8.02).abs() < 1e-4);
    }

    /// A sky at `time` of the day on the world's `days`.
    fn on_day(time: f32, days: f32) -> Sky {
        let mut sky = Sky::new(time, 900.0);
        sky.on_time_sync(time, days);
        sky
    }

    #[test]
    fn a_moonless_midnight_is_darker_than_one_under_a_full_moon_and_both_are_night() {
        // Day 1 is full and day 5 new (`moon::START`).
        let (full, new) = (on_day(0.0, 1.0), on_day(0.0, 5.0));
        assert!(full.moonlight() > 0.9, "a full moon at midnight gives {}", full.moonlight());
        assert!(new.moonlight() < 0.02, "a new moon at midnight gives {}", new.moonlight());
        assert!(new.sun_intensity() < full.sun_intensity() * 0.5, "the moonless night is not the darker one");
        assert!(new.night_floor() < 0.5 && full.night_floor() > 1.3, "floors {} and {}", new.night_floor(), full.night_floor());
        assert!(full.sun_intensity() < 0.15, "a full moon made a day of the night: {}", full.sun_intensity());
    }

    #[test]
    fn the_full_moon_stands_opposite_the_sun_and_the_new_moon_is_up_by_day() {
        assert!(-on_day(0.0, 1.0).moon_direction().y > 0.9, "the full moon is not high at midnight");
        assert!(-on_day(0.0, 5.0).moon_direction().y < -0.9, "the new moon is up at midnight");
        let noon_near_new = on_day(0.5, 5.5);
        assert!(-noon_near_new.moon_direction().y > 0.8, "the new moon is not up with the sun");
        // ...and a bright moon still under the horizon lights nothing: after
        // sunset on day 2 the waning moon is more than half lit and has not
        // risen, so the start of that night is dark.
        let before_moonrise = on_day(0.8, 2.8);
        assert!(before_moonrise.sun_elevation() < 0.0, "the sun is still up");
        assert!(primitive_shared::moon::illumination(before_moonrise.world_days()) > 0.5, "the moon is not a bright one");
        assert!(-before_moonrise.moon_direction().y < 0.0, "the moon has already risen");
        assert!(before_moonrise.moonlight() < 1e-3, "a moon under the horizon lights the ground: {}", before_moonrise.moonlight());
    }

    #[test]
    fn a_moonless_sky_is_darker_than_one_with_a_full_moon_in_it() {
        let (full, new) = (on_day(0.0, 1.0).clear_sky_color(), on_day(0.0, 5.0).clear_sky_color());
        assert!(new.length() < full.length() * 0.6, "the moonless sky {new:?} is not darker than the full moon's {full:?}");
        // Noon's sky has no moon in it.
        assert_eq!(on_day(0.5, 1.5).clear_sky_color(), on_day(0.5, 5.5).clear_sky_color());
    }

    #[test]
    fn by_day_the_moon_leaves_the_light_as_it_was() {
        for days in [1.5, 5.5, 3.5] {
            let noon = on_day(0.5, days);
            assert!((noon.night_floor() - 1.0).abs() < 1e-6, "the moon moved the floor at noon on day {days}");
        }
        assert!((on_day(0.5, 1.5).sun_intensity() - on_day(0.5, 5.5).sun_intensity()).abs() < 1e-6);
    }

    #[test]
    fn the_sky_glides_onto_a_drift_and_jumps_onto_a_difference() {
        let mut sky = Sky::new(0.30, 900.0);
        sky.on_time_sync(0.305, 0.305);
        assert_eq!(
            sky.time_of_day, 0.30,
            "a five-thousandth correction was jumped rather than eased"
        );

        let mut sky = Sky::new(0.30, 900.0);
        sky.on_time_sync(0.80, 0.80);
        assert!(
            (sky.time_of_day - 0.80).abs() < 1e-6,
            "half a day of correction was eased: the sun slid across the sky"
        );

        // ...and midnight is a seam, not an edge: a hundredth before
        // 0.00 is a hundredth away, not most of a day away.
        let mut sky = Sky::new(0.005, 900.0);
        sky.on_time_sync(0.995, 0.995);
        assert!(
            (sky.time_of_day - 0.005).abs() < 1e-6,
            "a correction across midnight was read as most of a day and jumped"
        );
    }

    use super::*;

    const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

    fn at(time: f32) -> Sky {
        Sky::new(time, 600.0)
    }

    /// **The property the whole design rests on.** Both light colours
    /// carry hue and nothing else, so turning coloured light on changed
    /// what the world looks like without changing how bright it is. A
    /// colour that drifted off luminance one would be a brightness
    /// change smuggled in as a colour change -- which is exactly the
    /// kind of thing that gets "fixed" later by turning the ambient up.
    #[test]
    fn both_light_colours_carry_hue_and_not_brightness() {
        for step in 0..48 {
            let sky = at(step as f32 / 48.0);
            for (what, colour) in [("sun", sky.sun_color()), ("fill", sky.fill_color())] {
                let luminance = colour.dot(LUMA);
                assert!(
                    (luminance - 1.0).abs() < 1e-3,
                    "{what} at {step}/48 has luminance {luminance}",
                    what = what
                );
                assert!(colour.min_element() > 0.0, "{what} went negative");
                assert!(colour.is_finite(), "{what} is not a number");
            }
        }
    }

    /// Sunrise is orange, noon is nearly white, and night is cool. If
    /// these three stop being true the sky is still lit -- just lit by
    /// nothing in particular, which is where this started.
    #[test]
    fn the_light_is_warm_low_down_and_cool_after_dark() {
        let warmth = |c: Vec3| c.x - c.z;

        // Walk the day and take the extremes rather than guessing which
        // hour is which: the elevation curve is the thing under test,
        // and hard-coding a time here would test the constant instead.
        let mut warmest = f32::MIN;
        let mut coolest = f32::MAX;
        let mut noon = 0.0;
        for step in 0..96 {
            let sky = at(step as f32 / 96.0);
            let w = warmth(sky.sun_color());
            warmest = warmest.max(w);
            coolest = coolest.min(w);
            if sky.sun_elevation() > 0.9 {
                noon = w;
            }
        }
        assert!(warmest > 0.7, "nothing in the day was warm (best {warmest})");
        assert!(coolest < -0.2, "nothing was cool (best {coolest})");
        assert!(
            noon.abs() < 0.25,
            "the midday sun is tinted {noon}, which is a filter rather than a sun"
        );
    }

    /// The property `both_light_colours_carry_hue_and_not_brightness`
    /// holds, at every step: the warmer key and the cooler fill change
    /// what colour the world is and never how bright, so turning the
    /// setting up moved the exposure of the game by nothing.
    #[test]
    fn every_lighting_step_changes_the_colour_of_the_light_and_not_its_brightness() {
        use crate::engine::lighting::Quality;
        for quality in Quality::ALL {
            for step in 0..96 {
                let sky = at(step as f32 / 96.0);
                for (what, colour) in [("sun", sky.sun_color_for(quality)), ("fill", sky.fill_color_for(quality))] {
                    let luminance = colour.dot(LUMA);
                    assert!(
                        (luminance - 1.0).abs() < 1e-3,
                        "{quality:?} {what} at {step}/96 has luminance {luminance}"
                    );
                    assert!(colour.min_element() > 0.0 && colour.is_finite(), "{quality:?} {what} at {step}/96 is {colour}");
                }
            }
        }
    }

    /// Warm is what was asked for; a yellow filter over noon is not.
    #[test]
    fn the_noon_sun_is_warm_and_not_a_filter_at_every_step() {
        use crate::engine::lighting::Quality;
        let noon = at(0.5);
        for quality in Quality::ALL {
            let sun = noon.sun_color_for(quality);
            let warmth = sun.x - sun.z;
            assert!(warmth > 0.0, "{quality:?}: the noon sun is cool ({sun})");
            assert!(warmth < 0.25, "{quality:?}: the noon sun is tinted {warmth}, which is a filter");
        }
        // ...and past Simple the fill is cooler than the key, which is
        // the whole of what makes a shaded face read as shade.
        for quality in [Quality::Balanced, Quality::High] {
            let (sun, fill) = (noon.sun_color_for(quality), noon.fill_color_for(quality));
            assert!(fill.z - fill.x > sun.z - sun.x + 0.2, "{quality:?}: the shade is not cooler than the sun");
        }
    }

    /// The flat noon sky the player asked for survives every step; what
    /// the setting changes is the evening.
    #[test]
    fn the_noon_sky_is_the_same_flat_blue_at_every_step() {
        use crate::engine::lighting::Quality;
        for quality in Quality::ALL {
            assert_eq!(at(0.5).sky_color_for(quality), at(0.5).sky_color(), "{quality:?} repainted noon");
            assert_eq!(at(0.5).horizon_glow(quality).w, 0.0, "{quality:?} put a sunset at noon");
        }
    }

    /// Simple adds nothing; the glow belongs to a low sun and nowhere
    /// else; the halo belongs to the top step.
    #[test]
    fn the_sunset_glow_is_a_low_sun_and_nothing_else() {
        use crate::engine::lighting::Quality;
        use primitive_shared::weather::Weather;
        let sunset = at(0.75);
        assert_eq!(sunset.horizon_glow(Quality::Simple), glam::Vec4::ZERO);
        assert_eq!(sunset.sun_haze(Quality::Simple), glam::Vec4::ZERO);
        assert_eq!(sunset.sun_haze(Quality::Balanced), glam::Vec4::ZERO, "the halo is a High purchase");
        assert!(sunset.sun_haze(Quality::High).truncate().length() > 0.1, "High has no halo at sunset");

        let glow = sunset.horizon_glow(Quality::Balanced);
        assert!(glow.w > 0.6, "sunset has almost no glow: {glow}");
        assert!(glow.x > glow.y && glow.y > glow.z, "the sunset glow is not orange: {glow}");
        assert_eq!(at(0.0).horizon_glow(Quality::Balanced).w, 0.0, "midnight glows");

        // Weather is the first thing to take it.
        let mut storm = at(0.75);
        storm.set_weather(Weather::Storm);
        for _ in 0..400 {
            storm.tick(0.05);
            storm.time_of_day = 0.75;
        }
        assert!(storm.horizon_glow(Quality::Balanced).w < 0.01, "a storm kept its sunset");
        assert!(storm.sun_haze(Quality::High).truncate().length() < 0.01, "a storm kept its halo");
    }

    /// The whole reason the evening sky changed: Simple paints every
    /// direction orange while the sun sets, and past it the side away
    /// from the sun keeps a colour of its own.
    #[test]
    fn the_evening_sky_away_from_the_sun_is_not_orange() {
        use crate::engine::lighting::Quality;
        let sunset = at(0.75);
        let simple = sunset.sky_color_for(Quality::Simple);
        let balanced = sunset.sky_color_for(Quality::Balanced);
        assert!(simple.x > simple.z * 2.0, "the Simple sunset is no longer the orange this was measured against");
        assert!(balanced.z >= balanced.x, "the sky behind a Balanced sunset is orange: {balanced}");
    }

    /// Overcast has no beam to be warm. The colour goes with the
    /// brightness rather than leaving a grey sky lit by an orange sun,
    /// which is the arrangement that reads as a bug.
    #[test]
    fn a_storm_takes_the_colour_out_of_the_light() {
        let mut clear = at(0.25);
        let mut storm = at(0.25);
        storm.set_weather(primitive_shared::weather::Weather::Storm);
        // The overcast figure is eased, so it has to be given time to
        // arrive -- see `set_weather`.
        for _ in 0..2000 {
            clear.tick(0.05);
            storm.tick(0.05);
        }
        let spread = |c: Vec3| c.max_element() - c.min_element();
        assert!(
            spread(storm.sun_color()) < spread(clear.sun_color()) + 1e-3,
            "a storm was more strongly coloured than a clear sky"
        );
    }

    /// The light never jumps, and least of all at sunset.
    ///
    /// **It used to jump hardest exactly there.** The curve is written
    /// in two halves, one for the sun above the horizon and one for
    /// below, and they were built from different numbers: the lower
    /// half faded from 0.35 down to the night floor, the upper climbed
    /// from the *night floor* up to noon. So the instant the sun
    /// touched zero the light fell to 0.09 and the next instant it was
    /// back at 0.35 -- a hole in the daylight, a fifth of a second wide
    /// at this game's day length, at the one moment a player is looking
    /// at the sky. Reported as "the evening darkens too abruptly".
    ///
    /// Asserted as a bound on the step rather than as a value, because
    /// the value is a design decision and the smoothness is not. A
    /// thousand samples is one every 0.9 seconds of real time at the
    /// default day length; a step of a twentieth across one of those is
    /// already a visible change, and the old seam was five times that.
    #[test]
    fn the_daylight_never_steps_and_the_worst_of_it_is_not_at_sunset() {
        const SAMPLES: usize = 1000;
        const MOST: f32 = 0.05;

        let light_at = |t: f32| Sky::new(t, 900.0).clear_sun_intensity();

        let mut worst = (0.0_f32, 0.0_f32);
        let mut previous = light_at(0.0);
        for i in 1..=SAMPLES {
            let t = i as f32 / SAMPLES as f32;
            let now = light_at(t);
            let step = (now - previous).abs();
            if step > worst.1 {
                worst = (t, step);
            }
            previous = now;
        }
        assert!(
            worst.1 <= MOST,
            "the light steps by {} at {} of the day, which reads as a flicker",
            worst.1,
            worst.0,
        );

        // ...and specifically not at the horizon, which is where the
        // two halves meet and where the seam was.
        let before = light_at(0.749);
        let at = light_at(0.750);
        let after = light_at(0.751);
        assert!(
            before >= at && at >= after,
            "the light rises again at sunset: {before} then {at} then {after}",
        );
    }

    #[test]
    fn noon_is_bright_and_midnight_is_not() {
        let noon = Sky::new(0.5, 600.0);
        let midnight = Sky::new(0.0, 600.0);
        assert!(noon.sun_intensity() > 0.9);
        assert!(midnight.sun_intensity() < 0.15);
        assert!(noon.sun_elevation() > 0.99);
        assert!(midnight.sun_elevation() < -0.99);
    }

    #[test]
    fn the_sun_is_overhead_at_noon() {
        let noon = Sky::new(0.5, 600.0);
        // Light travels downward at noon.
        assert!(noon.sun_direction().y < -0.9);
    }

    #[test]
    fn time_wraps_instead_of_running_away() {
        let mut sky = Sky::new(0.99, 10.0);
        sky.tick(1.0);
        assert!((0.0..1.0).contains(&sky.time_of_day), "{}", sky.time_of_day);
    }

    #[test]
    fn weather_arrives_over_seconds_rather_than_between_two_frames() {
        use primitive_shared::weather::Weather;
        let mut sky = Sky::new(0.5, 100_000.0);
        sky.set_weather(Weather::Storm);
        assert_eq!(sky.overcast(), 0.0, "the message is instant, the sky is not");
        sky.tick(1.0);
        let after_a_second = sky.overcast();
        assert!(
            after_a_second > 0.0 && after_a_second < 0.5,
            "a front that arrives in a second is a switch: {after_a_second}"
        );
        for _ in 0..60 {
            sky.tick(0.5);
        }
        assert!((sky.overcast() - 1.0).abs() < 1e-3, "it never got there: {}", sky.overcast());
        // ...and it clears again, rather than only ever getting darker.
        sky.set_weather(Weather::Clear);
        for _ in 0..60 {
            sky.tick(0.5);
        }
        assert!(sky.overcast() < 1e-3, "the storm never lifted: {}", sky.overcast());
    }

    #[test]
    fn a_storm_darkens_the_ground_and_not_only_the_sky() {
        // The half of `set_weather`'s promise that was never written.
        use primitive_shared::weather::Weather;
        let mut clear = Sky::new(0.5, 100_000.0);
        let mut storm = Sky::new(0.5, 100_000.0);
        storm.set_weather(Weather::Storm);
        for _ in 0..120 {
            clear.tick(0.5);
            storm.tick(0.5);
        }
        assert!(
            storm.sun_intensity() < clear.sun_intensity() * 0.7,
            "a storm at noon is as bright as a clear one: {} vs {}",
            storm.sun_intensity(),
            clear.sun_intensity()
        );
        // ...and it is still daylight. A storm is gloomy, not night.
        assert!(storm.sun_intensity() > Sky::new(0.0, 600.0).sun_intensity());
    }

    #[test]
    fn a_rainy_noon_is_not_darker_than_the_night_it_is_blended_over() {
        // The sky colour used to dim the storm grey by an intensity that
        // already had the storm in it. Cheap to get wrong, and it comes
        // out as a noon darker than dusk.
        use primitive_shared::weather::Weather;
        let mut storm = Sky::new(0.5, 100_000.0);
        storm.set_weather(Weather::Storm);
        for _ in 0..120 {
            storm.tick(0.5);
        }
        let midnight = Sky::new(0.0, 600.0);
        let brightness = |c: Vec3| c.x + c.y + c.z;
        assert!(
            brightness(storm.sky_color()) > brightness(midnight.sky_color()) * 2.0,
            "a storm at noon looks like midnight: {:?}",
            storm.sky_color()
        );
    }

    #[test]
    fn a_sync_across_the_midnight_wrap_takes_the_short_way() {
        let mut sky = Sky::new(0.99, 100_000.0);
        sky.on_time_sync(0.01, 0.01);
        for _ in 0..60 {
            sky.tick(1.0 / 60.0);
        }
        // Should have crossed 0.0 forward, not run backwards through noon.
        assert!(
            sky.time_of_day < 0.1 || sky.time_of_day > 0.98,
            "took the long way round: {}",
            sky.time_of_day
        );
    }

    #[test]
    fn dusk_takes_minutes_rather_than_three_quarters_of_one() {
        // **"Ночь быстро наступает".** Two numbers made it: how steeply the
        // light falls once the sun is down (`TWILIGHT_FALL`) and how long a
        // day is (`ServerSettings::day_length_seconds`). Written apart, each
        // looked defensible -- a physically right twilight, a brisk day --
        // and together they were a sunset a player could miss by looking at
        // their inventory. So this holds them against each other, in the
        // seconds somebody actually waits: at the eighth power and a
        // fifteen-minute day it was 45 seconds, and the pair is only right
        // when measured as a pair.
        let day = primitive_server::settings::ServerSettings::default().day_length_seconds;
        // How much of the light between the night floor and dusk is left:
        // one at the moment the sun touches the horizon, nought once the
        // world is down on the floor the moon leaves it (see `MOONLESS`).
        let left = |t: f32| {
            let sky = Sky::new(t, day);
            let night = NIGHT_INTENSITY * (MOONLESS + (FULL_MOON - MOONLESS) * sky.moonlight());
            (sky.clear_sun_intensity() - night) / (DUSK_INTENSITY - night)
        };
        let mut seconds = 0.0f32;
        while seconds < day && left(0.75 + seconds / day) > 0.05 {
            seconds += 1.0;
        }
        // A share of the day rather than seconds: the day has been fifteen,
        // twenty and now thirty minutes long, and the fall of the light
        // stretches with it -- 107 seconds at twenty minutes, 160 at thirty.
        // What has to hold is that it is minutes, never the 45 seconds of
        // a fifteen-minute day at the old power.
        assert!(
            (0.07 * day..=0.10 * day).contains(&seconds) && seconds >= 90.0,
            "dusk lasts {seconds} seconds of a {day}-second day; it was 45 when a player said the night falls too fast"
        );
        // And the ends of the day are where they were: this stretched the
        // middle of the fall, it did not lift the night or dim the noon.
        assert!((Sky::new(0.5, day).clear_sun_intensity() - 1.0).abs() < 1e-6, "noon moved");
        assert!(left(0.0) < 0.001, "midnight is no longer on the floor: {}", left(0.0));
    }

    /// A sky that has been in `weather` for `seconds`, stepped at thirty
    /// frames, starting clear.
    fn weathered(weather: primitive_shared::weather::Weather, seconds: f32, mut each: impl FnMut(&Sky)) -> Sky {
        let mut sky = on_day(0.5, 12.5);
        sky.set_weather(weather);
        for _ in 0..(seconds * 30.0) as usize {
            sky.tick(1.0 / 30.0);
            each(&sky);
        }
        sky
    }

    #[test]
    fn the_clouds_close_in_before_the_rain_falls_and_the_rain_stops_before_they_clear() {
        use primitive_shared::weather::Weather;
        let mut first_rain = None;
        let sky = weathered(Weather::Rain, 20.0, |sky| {
            if first_rain.is_none() && sky.rain_arrived() > 0.0 {
                first_rain = Some(sky.overcast());
            }
        });
        let covered = first_rain.expect("rain never arrived in twenty seconds of rain");
        assert!(
            covered >= Weather::Rain.intensity() * 0.45,
            "the first drops fell out of a sky only {covered:.2} overcast"
        );
        assert!((sky.rain_arrived() - 1.0).abs() < 1e-4, "the rain never reached full strength under a closed sky");

        let mut clearing = sky;
        clearing.set_weather(Weather::Clear);
        clearing.tick(1.0 / 30.0);
        assert_eq!(clearing.rain_arrived(), 0.0, "rain went on falling after the weather cleared");
        assert!(clearing.overcast() > 0.4, "the deck vanished with the rain instead of lingering");
    }

    #[test]
    fn the_clouds_run_with_the_wind_and_faster_in_a_storm_than_on_a_fair_day() {
        use primitive_shared::weather::Weather;
        let travelled = |weather: Weather| {
            let mut sky = on_day(0.5, 12.5);
            sky.set_weather(weather);
            let mut sum = glam::Vec2::ZERO;
            let mut path = 0.0;
            let mut last = sky.cloud_drift();
            for _ in 0..(20.0 * 30.0) as usize {
                sky.tick(1.0 / 30.0);
                // Unwrapped, so a step across `CLOUD_WRAP` is not a jump.
                let step = sky.cloud_drift() - last;
                let step = step - (step / CLOUD_WRAP).round() * CLOUD_WRAP;
                sum += step;
                path += step.length();
                last = sky.cloud_drift();
            }
            // The wind a moment in, which is what it was over the run to a
            // few per cent: it veers over hours.
            let wind = primitive_shared::raft::wind(on_day(0.5, 12.5).world_days() + 10.0 / 900.0, weather);
            (sum, path, wind)
        };
        let (fair, fair_path, fair_wind) = travelled(Weather::Clear);
        let (storm, storm_path, storm_wind) = travelled(Weather::Storm);
        assert!(storm_wind.strength > fair_wind.strength, "the storm's wind is no stronger than a fair day's");
        assert!(storm_path > fair_path, "a storm deck moved {storm_path} and a fair one {fair_path}");
        for (moved, path, wind) in [(fair, fair_path, fair_wind), (storm, storm_path, storm_wind)] {
            let expected = (CLOUD_CALM_SPEED + CLOUD_WIND_SPEED * wind.strength) * CLOUD_SCALE * 20.0;
            assert!(
                (path / expected - 1.0).abs() < 0.15,
                "the deck moved {path} in twenty seconds of a wind that carries it {expected}"
            );
            // The drift runs against the wind (the layer is sampled at world
            // plus drift), so the deck itself goes the wind's way.
            let (sin, cos) = wind.toward.sin_cos();
            let along = -moved.normalize().dot(glam::Vec2::new(cos, sin));
            assert!(along > 0.9, "the deck ran {along:.2} along the wind it is blown by");
        }
    }

    /// **A rain cloud is darker than a fair one, and there is more of it.**
    /// Read as source, for the reason the ambient floor's test is: the
    /// property is three lines of `sky.wgsl`, and a test that needs an adapter
    /// does not run in CI. `sky_repro` has the pictures.
    #[test]
    fn rain_clouds_are_darker_and_cover_more_of_the_sky_than_fair_ones() {
        let source = include_str!("sky.wgsl").replace("\r\n", "\n");
        let luma = |v: &str| {
            let parts: Vec<f32> = v.split(',').map(|p| p.trim().parse::<f32>().expect("a number")).collect();
            0.2126 * parts[0] + 0.7152 * parts[1] + 0.0722 * parts[2]
        };
        for name in ["sunlit", "shaded"] {
            let line = source
                .lines()
                .find(|l| l.trim_start().starts_with(&format!("let {name} = mix(")))
                .unwrap_or_else(|| panic!("no `{name}` cloud colour in sky.wgsl"));
            let vectors: Vec<&str> = line.split("vec3<f32>(").skip(1).map(|v| v.split(')').next().unwrap()).collect();
            assert_eq!(vectors.len(), 2, "`{name}` is not a mix of two colours: {line}");
            assert!(line.contains("overcast"), "`{name}` does not follow the weather: {line}");
            assert!(luma(vectors[1]) < luma(vectors[0]) * 0.6, "the {name} side of a rain cloud is not much darker: {line}");
        }
        assert!(
            source.contains("let cover = mix(globals.sky_params.y, 0.94, overcast);"),
            "the deck no longer spreads toward nearly solid under weather"
        );
        assert!(
            source.contains("let drift = vec2<f32>(globals.glow_dir.y, globals.glow_dir.w);"),
            "the deck no longer drifts on the wind the CPU integrates"
        );
    }
}
