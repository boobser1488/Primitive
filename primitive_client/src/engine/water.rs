//! **What colour water is**, in one place, because two things have to
//! agree about it and they run on different processors.
//!
//! The *surface* is painted by the fragment shader, per face, out of a code
//! the mesher packed (`worldgen::water_climate_of`, `WaterTint::code`). The
//! *murk* -- the colour a swimmer's distance lands on, which is also what
//! the sky pass paints under water -- is one colour a frame, worked out
//! here on the CPU and handed over as the fog colour (`fog::Fog`).
//!
//! They were allowed to disagree once, and the note where `fog` used to
//! keep its one underwater colour says what that looked like: the far bed
//! and the underside of the surface standing out of the water as flat
//! bright bands. So the palette is written once, here, and
//! `the_shader_paints_the_water_this_file_mixes` holds the shader's copy of
//! it to these numbers.
//!
//! ## What the colour is made of
//!
//! Three things, in this order:
//!
//! * **The climate**, as two smooth fields and never as a biome -- see
//!   `worldgen::water_climate_of` for why a classification would draw a
//!   line across the water at every boundary. One axis is how cold the
//!   water is; the other is what is suspended in it, peat at one end and
//!   rock flour at the other.
//! * **How deep the column under the face is**, which is the one the eye
//!   reads first: a sea is dark because you are looking down twenty
//!   blocks of it, and the same water two cells deep over sand is bright.
//!   The mesher already counts this per face (`liquid_depth_below`).
//! * **The sky on it**, which is a highlight and not a wash. See
//!   `WATER_SHEEN` in shader.wgsl.
//!
//! ## The numbers are linear albedo, and the picture is divided out
//!
//! `assets/textures/terrain/water.png` is one blue with a ripple in it
//! (`WATER_MEDIAN` is its middle, in linear light). The shader multiplies
//! a texel by the tint the vertex carries, so the tint handed over is
//! `body / WATER_MEDIAN`: the picture's *deviation* from its own middle
//! survives as the ripple, and the colour is entirely this file's. The
//! alternative -- a tint near one over a picture that already decides the
//! hue -- is what the water had, and it is why a marsh could not be brown:
//! reaching brown from that blue wanted a red multiplier of fourteen, and
//! a multiplier of fourteen turns the ripple into stripes.

use glam::Vec3;

/// The middle of `terrain/water.png`, in linear light.
///
/// The picture is 16x16 of one blue, dithered by a few values either way;
/// this is the colour 36 of its 256 texels are exactly and all of them are
/// within a few per cent of. sRGB (57, 107, 190) through the transfer
/// curve the atlas is sampled with (`Rgba8UnormSrgb`).
///
/// **It is a divisor and not a colour anybody chose.** If the picture is
/// ever redrawn, this number moves with it, and nothing else in the file
/// does -- which is the whole reason it is written down separately rather
/// than folded into eight constants.
const WATER_MEDIAN: Vec3 = Vec3::new(0.041, 0.147, 0.509);

/// **Deep water, cold**: a northern sea. Dark, and grey rather than blue --
/// the channels sit closer together than anywhere else in the palette,
/// which is what "grey" is. A player who swims north should be able to see
/// that the water got colder without being told.
const WATER_DEEP_COLD: Vec3 = Vec3::new(0.014, 0.038, 0.115);
/// **Deep water, warm**: the open sea in the tropics. Darker than the old
/// water and far more saturated -- blue is three quarters of it against a
/// fifth for green, where the picture alone was a third.
///
/// **Both ends of this axis are blue, deliberately.** The turquoise a
/// player expects of the tropics is a *shallow* colour and lives in
/// `WATER_SHOAL_TROPIC`; putting it here instead made every temperate lake
/// half tropical, because a temperate climate sits around 0.55 on this
/// axis and not at its end.
const WATER_DEEP_WARM: Vec3 = Vec3::new(0.012, 0.070, 0.330);

/// **What two cells of water over a bed does to the colour**, as a
/// multiplier, anywhere in the world: brighter, and greener than the
/// column it came off.
///
/// Short path, so little is absorbed; and what little colour there is
/// comes back off the bed rather than out of the depth. This is the term
/// that makes a shore read as a shore.
const WATER_SHOAL: Vec3 = Vec3::new(1.60, 1.90, 1.35);
/// The same, in water warm enough to be a lagoon.
///
/// **This is the tropical turquoise, and it is the one colour in the
/// palette that is a place rather than a climate.** Green is lifted three
/// times against blue's one and a half, which is what turns the shallows
/// over white sand that particular colour. Reached through a curve on the
/// warmth (`WATER_TROPIC_FROM`..`WATER_TROPIC_FULL`) so that it is the
/// tropics and not merely "the warm half of the map".
const WATER_SHOAL_TROPIC: Vec3 = Vec3::new(2.40, 3.20, 1.55);
/// How many cells of column it takes for the shoal to be gone: one cell is
/// all of it, six is none.
///
/// Six and not twenty, because this is the bed showing through and the bed
/// stops showing long before the water stops getting darker -- the far
/// half of that is `liquid_depth_below` through the alpha, which already
/// closes the window with depth.
const WATER_SHOAL_CELLS: f32 = 5.0;

/// **Peat, warm**: a marsh. Tea over a bed of dead leaves.
///
/// Red above green above blue, which is what brown is and what no amount
/// of tinting the old blue picture could reach.
const WATER_PEAT_WARM: Vec3 = Vec3::new(0.045, 0.030, 0.012);
/// **Peat, cold**: a bog. The same water with less light and less life in
/// it -- darker, and nearer olive than tea.
const WATER_PEAT_COLD: Vec3 = Vec3::new(0.026, 0.020, 0.011);

/// **Rock flour, warm**: a tarn high enough for the melt to carry powder,
/// in country that is not itself frozen. Green and blue level with each
/// other, which is turquoise, and both well up -- the point of flour is
/// that it *scatters*, so this water is brighter than clear water and you
/// can see less through it.
const WATER_FLOUR_WARM: Vec3 = Vec3::new(0.045, 0.150, 0.185);
/// **Rock flour, cold**: straight glacier melt. The same turquoise with
/// the light taken out of it.
const WATER_FLOUR_COLD: Vec3 = Vec3::new(0.038, 0.130, 0.170);

/// Where the shoal starts turning tropical, on the warmth axis, and where
/// it has finished.
///
/// **A long ramp with a late start, and both halves of that were forced.**
/// Starting at the cold end put a third of the lagoon colour into every
/// temperate pond and the water came out teal, which is precisely the
/// "голубая" this palette was rewritten to get away from. Making the ramp
/// *short* instead -- 0.60 to 0.88, which is what it first was -- fixed
/// that and broke the other end: the chill axis has thirty-two steps, so a
/// short ramp crosses it in three, and three steps of a green lifted three
/// times is a band drawn along a warm shore. Long and late does both.
const WATER_TROPIC_FROM: f32 = 0.45;
/// See `WATER_TROPIC_FROM`.
const WATER_TROPIC_FULL: f32 = 1.00;

/// Where the peat has stopped mattering, in cells of column under the
/// face.
///
/// **A marsh is shallow and a bay is not**, and that is the whole of how
/// this palette tells them apart without asking the continent spline --
/// see `worldgen::water_climate_of`. Ten cells, so a wet coast is brown
/// in its creeks and its mudflats and blue off the beach, and the change
/// between them is the shelf rather than a line.
const WATER_PEAT_SHALLOW: f32 = 2.0;
/// See `WATER_PEAT_SHALLOW`.
const WATER_PEAT_DEEP: f32 = 10.0;

/// How bright the murk is, as luminance.
///
/// The colour a swimmer's distance lands on is the *medium's own light*
/// and not how dark its albedo happens to be, so the water's hue is
/// brought to one level here rather than used at strength. The level is
/// the luminance the old flat `UNDERWATER` teal had, so a temperate lake
/// is exactly as bright to swim in as it was and only its colour moved.
const MURK_LEVEL: f32 = 0.137;
/// How far the murk is pulled toward its own grey.
///
/// Scattering is not a filter: a long look through water mixes in light
/// that came off everything, which washes the hue out. Without this a
/// swimmer in a marsh sat inside a saturated orange, which reads as a
/// bug rather than as peat.
const MURK_GREY: f32 = 0.25;

/// Rec. 709 luminance, the same weights the shader's air perspective uses.
const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

/// Steps of the chill axis and of the silt axis in a packed climate. Must
/// match `WATER_CHILL_STEPS` and `WATER_SILT_STEPS` in shader.wgsl, and
/// their product must fit in `mesh::WATER_CLIMATE_MASK`. See
/// `WaterTint::code` for why they are not the same number.
const CHILL_STEPS: u32 = 32;
/// See [`CHILL_STEPS`].
const SILT_STEPS: u32 = 512;
const _: () = assert!(CHILL_STEPS * SILT_STEPS - 1 <= crate::engine::mesh::WATER_CLIMATE_MASK);

/// The water of one place: what the climate made of it, and how much of it
/// there is under the face -- or under the swimmer.
///
/// Two smooth axes and a count of cells; see the module note for what each
/// does. The pair is what the vertex carries as a byte and what the fog
/// carries as a colour, which is why it is a type rather than two floats
/// passed around.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterTint {
    /// 0 tropical, 1 polar. `1 - temperature`, lapse rate included.
    pub chill: f32,
    /// 0 peat, 0.5 carrying nothing, 1 rock flour. See
    /// `worldgen::water_climate_of`.
    pub silt: f32,
    /// Cells of water under this face, as `mesh::liquid_depth_below`
    /// counts them: 1 is a face with nothing under it.
    pub depth: f32,
}

impl WaterTint {
    /// Plain water: temperate, carrying nothing, a few cells deep.
    ///
    /// **What a tool photographs in when it has no world to ask.**
    /// `renderer::draw_scene` is handed meshes and a camera and no
    /// generator, so it cannot know what water its camera is in; a tool
    /// that stands its eye in a marsh or a glacier lake says so with
    /// `draw_scene_in_water`, or it photographs the right surface over the
    /// wrong murk.
    pub const PLAIN: WaterTint = WaterTint { chill: 0.45, silt: 0.5, depth: 4.0 };

    /// The water of a column, from the generator both sides share.
    pub fn of_column(
        world: &primitive_shared::worldgen::WorldGen,
        gx: i32,
        gz: i32,
        surface_y: i32,
        depth: f32,
    ) -> Self {
        let (chill, silt) = world.water_climate(gx, gz, surface_y);
        WaterTint { chill, silt, depth }
    }

    /// **The water round a swimmer's head**, for the fog.
    ///
    /// The climate comes from the generator, because a climate is a
    /// property of a place; **how much water there is comes from the
    /// blocks the client actually holds**, because a player may have dug
    /// the pond themselves and the murk has to follow what is there rather
    /// than what the generator would have made. Counted downward from the
    /// eye and capped exactly as the mesher caps a lid's column
    /// (`mesh::MAX_WATER_DEPTH`) -- past two dozen cells nothing in the
    /// palette moves.
    pub fn around_the_eye(
        world: &primitive_shared::worldgen::WorldGen,
        block_at: impl Fn(i32, i32, i32) -> Option<primitive_shared::types::BlockId>,
        eye: glam::Vec3,
    ) -> Self {
        let (gx, gy, gz) = (eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32);
        let mut depth = 0u32;
        while depth < crate::engine::mesh::MAX_WATER_DEPTH
            && block_at(gx, gy - depth as i32, gz)
                .is_some_and(primitive_shared::types::is_liquid)
        {
            depth += 1;
        }
        Self::of_column(world, gx, gz, gy, depth.max(1) as f32)
    }

    /// The climate as a vertex carries it: fourteen bits in the
    /// coordinate word (`mesh::WATER_CLIMATE_SHIFT`), thirty-two steps of
    /// chill and five hundred and twelve of silt.
    ///
    /// **Not the byte the foliage tint uses, and not a square grid, and
    /// both of those are arithmetic rather than taste.**
    ///
    /// The byte first: fifteen steps an axis is invisible on grass
    /// because the grass palette is a hue shift over a picture that is
    /// already green. The water palette is the whole colour, and it runs
    /// from a marsh brown to an open blue -- one fifteenth of that moves
    /// the blue channel further than the channel is wide at the brown
    /// end, which is a band drawn across a marsh every dozen blocks.
    ///
    /// Then the shape: the two axes are not the same size *in colour*.
    /// Silt carries brown at one end and turquoise at the other with
    /// clear water in the middle, so it spends its steps over two
    /// half-ranges and needs four times what chill does -- chill only
    /// slides one blue along to another. 32 by 512 is the split that
    /// leaves both under four values out of 255 a step in sRGB, which is
    /// inside the dither already in the picture.
    ///
    /// The depth is not in here: it rides in the same word, counted in
    /// cells, because it is a fact about the *column* and not about the
    /// climate -- one body of water has one climate and a different depth
    /// at every face of it.
    pub fn code(&self) -> u32 {
        let quantise = |v: f32, steps: u32| {
            (v.clamp(0.0, 1.0) * (steps - 1) as f32).round() as u32
        };
        quantise(self.chill, CHILL_STEPS) * SILT_STEPS + quantise(self.silt, SILT_STEPS)
    }

    /// The pair back out of the byte, as the shader reads it.
    ///
    /// Quantised, so this is not the exact climate that went in -- which
    /// is the point: this is what the *picture* is painted from, and a
    /// test that compares the shader's colour with an unquantised one
    /// would be comparing two different waters.
    ///
    /// The decoder, mirroring what the shader does, as `mesh::Vertex`'s
    /// decoders do: nothing at run time reads it and the tests that walk
    /// the palette for seams do.
    #[allow(dead_code)]
    pub fn from_code(code: u32, depth: f32) -> Self {
        let index = code.min(CHILL_STEPS * SILT_STEPS - 1);
        WaterTint {
            chill: (index / SILT_STEPS) as f32 / (CHILL_STEPS - 1) as f32,
            silt: (index % SILT_STEPS) as f32 / (SILT_STEPS - 1) as f32,
            depth,
        }
    }

    /// **The colour of the water itself**, in linear light, before any
    /// light falls on it and before the sky is reflected in it.
    ///
    /// This is the function the shader's `water_body` is a copy of, and
    /// `the_shader_paints_the_water_this_file_mixes` is what holds the two
    /// together.
    pub fn body(&self) -> Vec3 {
        let warmth = 1.0 - self.chill.clamp(0.0, 1.0);
        let silt = self.silt.clamp(0.0, 1.0);
        // Peat below the middle of the axis, flour above it, and nothing
        // at all in the middle -- which is why this is two half-ranges and
        // not a mix across one square. A square would have put the average
        // of brown and turquoise in the middle, and the average of brown
        // and turquoise is mud.
        let flour = (silt * 2.0 - 1.0).clamp(0.0, 1.0);
        // ...and peat only in water shallow enough to be a marsh. See
        // `WATER_PEAT_SHALLOW`.
        let peat = (1.0 - silt * 2.0).clamp(0.0, 1.0)
            * (1.0 - ramp(WATER_PEAT_SHALLOW, WATER_PEAT_DEEP, self.depth));

        let clear = WATER_DEEP_COLD.lerp(WATER_DEEP_WARM, warmth);
        let mut body = clear.lerp(WATER_PEAT_COLD.lerp(WATER_PEAT_WARM, warmth), peat);
        body = body.lerp(WATER_FLOUR_COLD.lerp(WATER_FLOUR_WARM, warmth), flour);

        // The bed, showing through what little column there is.
        let shallow = (1.0 - (self.depth - 1.0) / WATER_SHOAL_CELLS).clamp(0.0, 1.0);
        let tropic = ramp(WATER_TROPIC_FROM, WATER_TROPIC_FULL, warmth);
        let shoal = Vec3::ONE.lerp(WATER_SHOAL.lerp(WATER_SHOAL_TROPIC, tropic), shallow);
        body * shoal
    }

    /// The tint a vertex hands the fragment shader: the body colour with
    /// the picture's own middle divided out. See the module note.
    ///
    /// The shader does this division itself, at the end of `water_body`,
    /// so nothing on the CPU needs the answer -- what needs it is the
    /// tools and the tests, which print and check the number the GPU is
    /// actually going to multiply by.
    #[allow(dead_code)]
    pub fn over_the_picture(&self) -> Vec3 {
        self.body() / WATER_MEDIAN
    }

    /// **What a long look through this water lands on**: the fog colour of
    /// a submerged frame, and the colour the sky pass paints there.
    ///
    /// Not the surface colour -- looking *through* water is a longer path
    /// than looking *at* it, and it is the medium's own scattered light
    /// rather than its albedo, so the hue is the water's and the
    /// brightness is fixed (`MURK_LEVEL`).
    pub fn murk(&self) -> Vec3 {
        let body = self.body();
        let luma = body.dot(LUMA).max(1e-5);
        let hue = body * (MURK_LEVEL / luma);
        hue.lerp(Vec3::splat(MURK_LEVEL), MURK_GREY)
    }
}

/// The same Hermite ramp `worldgen::water_climate_of` and WGSL's
/// `smoothstep` are, so all three mean one curve.
fn ramp(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A colour, near enough that nobody could see the difference.
    fn close(a: Vec3, b: Vec3, what: &str) {
        assert!(
            (a - b).abs().max_element() < 1e-4,
            "{what}: {a:?} against {b:?}"
        );
    }

    /// **The shader mixes the water out of the same eight colours this
    /// file does**, and out of the same three curves.
    ///
    /// Written because the two copies are on different processors and
    /// nothing links them: the surface is painted in WGSL and the murk
    /// a swimmer sees at distance is painted from here, and when they
    /// disagreed the last time the far bed stood out of the water in
    /// flat bright bands (see `fog::UNDERWATER`). This parses the
    /// shader's own text for every constant the palette is made of and
    /// fails the build when one of them moves alone.
    #[test]
    fn the_shader_paints_the_water_this_file_mixes() {
        let source = include_str!("shader.wgsl");
        let vector = |name: &str| -> Vec3 {
            let head = format!("const {name}: vec3<f32> = vec3<f32>(");
            let at = source
                .find(&head)
                .unwrap_or_else(|| panic!("shader.wgsl has no {name}"))
                + head.len();
            let body = &source[at..];
            let body = &body[..body.find(')').expect("a vec3 that is never closed")];
            let parts: Vec<f32> = body
                .split(',')
                .map(|n| n.trim().parse().expect("a vec3 component that is not a number"))
                .collect();
            assert_eq!(parts.len(), 3, "{name} is not three numbers");
            Vec3::new(parts[0], parts[1], parts[2])
        };
        let scalar = |name: &str| -> f32 {
            let head = format!("const {name}: f32 = ");
            let at = source
                .find(&head)
                .unwrap_or_else(|| panic!("shader.wgsl has no {name}"))
                + head.len();
            let body = &source[at..];
            body[..body.find(';').expect("a constant that is never ended")]
                .trim()
                .parse()
                .expect("a constant that is not a number")
        };
        close(vector("WATER_MEDIAN"), WATER_MEDIAN, "WATER_MEDIAN");
        close(vector("WATER_DEEP_COLD"), WATER_DEEP_COLD, "WATER_DEEP_COLD");
        close(vector("WATER_DEEP_WARM"), WATER_DEEP_WARM, "WATER_DEEP_WARM");
        close(vector("WATER_SHOAL"), WATER_SHOAL, "WATER_SHOAL");
        close(vector("WATER_SHOAL_TROPIC"), WATER_SHOAL_TROPIC, "WATER_SHOAL_TROPIC");
        close(vector("WATER_PEAT_WARM"), WATER_PEAT_WARM, "WATER_PEAT_WARM");
        close(vector("WATER_PEAT_COLD"), WATER_PEAT_COLD, "WATER_PEAT_COLD");
        close(vector("WATER_FLOUR_WARM"), WATER_FLOUR_WARM, "WATER_FLOUR_WARM");
        close(vector("WATER_FLOUR_COLD"), WATER_FLOUR_COLD, "WATER_FLOUR_COLD");
        for (name, ours) in [
            ("WATER_SHOAL_CELLS", WATER_SHOAL_CELLS),
            ("WATER_TROPIC_FROM", WATER_TROPIC_FROM),
            ("WATER_TROPIC_FULL", WATER_TROPIC_FULL),
            ("WATER_PEAT_SHALLOW", WATER_PEAT_SHALLOW),
            ("WATER_PEAT_DEEP", WATER_PEAT_DEEP),
        ] {
            assert!((scalar(name) - ours).abs() < 1e-5, "{name} differs from the shader's");
        }
    }

    /// **Water is water in every climate**: blue where nothing is
    /// suspended in it, brown in a marsh, turquoise off a glacier -- and
    /// never a colour that is none of those.
    ///
    /// Stated as the relations between the channels rather than as
    /// numbers, because the numbers are a palette somebody will tune and
    /// the relations are what makes each of them the thing it is named
    /// after. A marsh whose blue channel crept over its red is not a
    /// marsh any more, however good it looks beside the last one.
    #[test]
    fn every_water_in_the_palette_is_the_water_it_is_named_after() {
        let sea = WaterTint { chill: 0.4, silt: 0.5, depth: 20.0 }.body();
        assert!(sea.z > sea.y * 3.0 && sea.y > sea.x, "the open sea is not deep blue: {sea:?}");
        let north = WaterTint { chill: 0.95, silt: 0.5, depth: 20.0 }.body();
        assert!(north.length() < sea.length(), "northern water is not darker: {north:?}");
        assert!(
            north.z / north.y < sea.z / sea.y,
            "northern water is not greyer than the warm sea: {north:?} against {sea:?}"
        );
        let marsh = WaterTint { chill: 0.35, silt: 0.0, depth: 1.0 }.body();
        assert!(marsh.x > marsh.y && marsh.y > marsh.z, "a marsh is not brown: {marsh:?}");
        let bog = WaterTint { chill: 0.85, silt: 0.0, depth: 1.0 }.body();
        assert!(bog.x > bog.z && bog.length() < marsh.length(), "a bog is not dark peat: {bog:?}");
        let glacier = WaterTint { chill: 0.85, silt: 1.0, depth: 6.0 }.body();
        assert!(
            glacier.y > glacier.z * 0.6 && glacier.y > sea.y,
            "glacier melt is not the milky turquoise it should be: {glacier:?}"
        );
        let lagoon = WaterTint { chill: 0.05, silt: 0.5, depth: 1.0 }.body();
        assert!(
            lagoon.y > lagoon.z * 0.4 && lagoon.y > sea.y * 2.0,
            "tropical shallows are not turquoise: {lagoon:?}"
        );
    }

    /// **Water that carries nothing gets darker the deeper it stands**,
    /// at every climate, and never the other way round.
    ///
    /// The depth is the term a player reads first -- a sea is dark
    /// because there is twenty blocks of it -- and a palette that only
    /// moved with the climate would draw a bay and the creek feeding it
    /// as one colour.
    ///
    /// **Why the peat half is not in here.** Peat fades out with the
    /// column (`WATER_PEAT_SHALLOW`), because that is what tells a marsh
    /// from the bay it drains into; so a creek deepening into a channel
    /// changes *water* as it goes, from a dark brown to an open blue that
    /// is genuinely brighter. Demanding a monotone luminance across that
    /// would be demanding that the sea be darker than a bog, which it is
    /// not. What is asserted about that half is in
    /// `a_marsh_is_brown_where_it_is_shallow_and_blue_where_it_is_a_bay`.
    #[test]
    fn a_column_of_clear_water_darkens_the_deeper_it_stands() {
        for chill in [0.0f32, 0.3, 0.6, 0.9] {
            for silt in [0.5f32, 0.75, 1.0] {
                let luma = |depth: f32| WaterTint { chill, silt, depth }.body().dot(LUMA);
                let mut last = f32::INFINITY;
                for cells in 1..=24 {
                    let now = luma(cells as f32);
                    assert!(
                        now <= last + 1e-6,
                        "water at ({chill}, {silt}) got brighter from {} to {cells} cells",
                        cells - 1
                    );
                    last = now;
                }
                assert!(
                    luma(1.0) > luma(20.0) * 1.3,
                    "a puddle at ({chill}, {silt}) is no lighter than twenty blocks of sea"
                );
            }
        }
    }

    /// **A marsh is brown where it is shallow and the bay it drains into
    /// is blue**, and there is no line between them.
    ///
    /// This is the other half of the depth term, and the whole of how the
    /// palette tells a creek from the sea without asking the continent
    /// spline -- see `worldgen::water_climate_of`.
    #[test]
    fn a_marsh_is_brown_where_it_is_shallow_and_blue_where_it_is_a_bay() {
        let marsh = WaterTint { chill: 0.35, silt: 0.0, depth: 1.0 };
        assert!(marsh.body().x > marsh.body().z, "the shallow end is not brown");
        let bay = WaterTint { depth: 20.0, ..marsh };
        let open = WaterTint { silt: 0.5, ..bay };
        assert!(
            (bay.body() - open.body()).abs().max_element() < 1e-4,
            "twenty cells of the same climate is still peaty: {:?}",
            bay.body()
        );
    }

    /// **No step anywhere in the palette**: one bucket of climate, or one
    /// cell of depth, never moves the colour by more than the dither
    /// already in the picture.
    ///
    /// This is the seam test. The fields underneath are smooth
    /// (`worldgen::water_climate_of`), so the only way a line can appear
    /// on a lake is a palette with a jump in it -- a threshold somebody
    /// wrote as an `if`, a corner whose neighbours are a long way apart,
    /// or an axis given too few steps to say what it has to say. All
    /// three would pass every other test in this file, and the third is
    /// what took the climate out of the tint byte (`WaterTint::code`).
    ///
    /// **Measured in sRGB and not in linear light**, because that is what
    /// the eye is handed and because the peat end of the palette lives
    /// where the transfer curve is steepest: a linear step of a hundredth
    /// is nothing in a bright blue and a visible band in a dark brown.
    /// The bar is 0.015, which is under four values out of 255 -- less
    /// than the +-6 `terrain/water.png` already dithers its own blue by,
    /// and the reason the whole climate moved out of the tint byte, where
    /// the worst step was 0.073 and drew visible bands across a marsh.
    #[test]
    fn crossing_a_climate_boundary_never_steps_the_colour_of_the_water() {
        // The transfer curve the atlas is sampled through, in the
        // direction the eye sees.
        let shown = |c: Vec3| -> Vec3 {
            Vec3::new(c.x, c.y, c.z).powf(1.0 / 2.2)
        };
        let step_of = |here: &WaterTint, there: &WaterTint| {
            (shown(here.body()) - shown(there.body())).abs().max_element()
        };
        for depth in [1.0f32, 2.0, 5.0, 12.0, 24.0] {
            // Every bucket of the packing, against its neighbour along
            // each axis -- which is the pair of columns either side of a
            // boundary in the world.
            for code in 0..CHILL_STEPS * SILT_STEPS {
                let here = WaterTint::from_code(code, depth);
                for step in [1, SILT_STEPS] {
                    if (code % SILT_STEPS == SILT_STEPS - 1 && step == 1)
                        || code + step >= CHILL_STEPS * SILT_STEPS
                    {
                        continue;
                    }
                    let there = WaterTint::from_code(code + step, depth);
                    let jump = step_of(&here, &there);
                    assert!(
                        jump < 0.015,
                        "one climate step at depth {depth} moves the water by {jump}: \
                         {:?} against {:?}",
                        here.body(),
                        there.body()
                    );
                }
            }
        }
        // ...and one cell of column, which is the other boundary: the
        // shelf a bay shallows over.
        //
        // **A looser bar, on purpose.** A climate bucket is a slice of a
        // smooth field and a step in it has no business being visible; a
        // cell of water is a whole block, and the bed under it steps by a
        // whole block at the same place. The colour is *supposed* to move
        // there -- that is the depth the player is being shown -- so what
        // is asserted is only that it moves by a shade and not by a
        // palette entry.
        for silt in [0.0f32, 0.5, 1.0] {
            for cells in 1..24 {
                let jump = step_of(
                    &WaterTint { chill: 0.5, silt, depth: cells as f32 },
                    &WaterTint { chill: 0.5, silt, depth: cells as f32 + 1.0 },
                );
                assert!(jump < 0.08, "the shelf at {cells} cells steps the colour by {jump}");
            }
        }
    }

    /// **A swimmer can see, and what they see is the water they dived
    /// into.**
    ///
    /// Two failures, both of which have happened to this game: a murk
    /// bright enough to be a glowing box under a dark surface, and one so
    /// dark that being under water is being in a cave. The luminance is
    /// pinned, so every water in the palette is exactly as much to swim
    /// in as the old flat teal was, and only the colour moves.
    #[test]
    fn under_every_water_in_the_world_a_swimmer_can_still_see() {
        for chill in [0.0f32, 0.5, 1.0] {
            for silt in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                for depth in [1.0f32, 6.0, 24.0] {
                    let murk = WaterTint { chill, silt, depth }.murk();
                    let luma = murk.dot(LUMA);
                    assert!(
                        (luma - MURK_LEVEL).abs() < 1e-3,
                        "({chill}, {silt}, {depth}) is {luma} bright to swim in"
                    );
                    assert!(
                        murk.min_element() > 0.0,
                        "({chill}, {silt}, {depth}) has a dead channel: {murk:?}"
                    );
                }
            }
        }
        // ...and the murk is the water's own colour and not one colour for
        // everything, which is what it used to be.
        let marsh = WaterTint { chill: 0.35, silt: 0.0, depth: 1.0 }.murk();
        let sea = WaterTint { chill: 0.35, silt: 0.5, depth: 20.0 }.murk();
        assert!(marsh.x > sea.x * 2.0, "a marsh and the sea are the same murk");
    }
}
