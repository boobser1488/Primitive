//! Which way a river's water is going, and how fast.
//!
//! ## What was asked
//!
//! "Сделай быстрые реки с течением": a river that carries what is in it,
//! lazy on the flat and a rapid where the country falls, so that crossing one
//! is a choice of where -- a calm reach, a ford -- and not a swim straight
//! over wherever the path met the bank.
//!
//! ## Where the current comes from
//!
//! **Chosen: the fall of the country along the channel.** Every river here
//! holds its water at the sea's level (`scale::EARTH_RIVERS`), so the water's
//! own surface is flat from its mouth to where it peters out in the hills and
//! says nothing about which way is down. The country it was cut through does:
//! the ground before the rivers (`WorldGen::land_before_rivers`) is read a
//! little way up and down the channel, and the water runs toward the lower
//! end, as fast as that ground falls. A river through a plain is a slow
//! slide, and where it leaves the hills through a steep valley it is a rapid.
//!
//! The direction is the channel's own -- square to the river field's slope,
//! which is what the channel is cut along -- multiplied by the fall along it,
//! so it has no sign to choose: turned the other way along the channel, the
//! fall changes sign with it and the product does not. That is also what
//! keeps it continuous. Where the country along a river rises and falls
//! again the water runs down both sides into the low place and stands there:
//! a pool between two reaches, with slack water in it, which a real river has
//! wherever its bed levels out.
//!
//! **Strongest in the middle of the channel and slack at the bank**, in
//! proportion to how deep into the channel a column is. A swimmer who keeps
//! to the edge of a rapid can work along it; one who strikes out across the
//! middle is taken.
//!
//! **Worked out from the generator on both sides of the wire**, as the sea's
//! circulation is from the clock (`fluid::current_at`): the client predicting
//! a swimmer or a raft and the server moving a raft or a dropped stick ask
//! the same seed the same question, and nothing is sent.
//!
//! Rejected:
//! * *The fluid simulation's own flow.* A raft already feels water running
//!   from a fuller cell into an emptier one (`raft::current`), but a river
//!   here is a channel of full cells at one level -- there is no difference
//!   to feel, and making one would be making the water drain.
//! * *Water that actually falls: a river stepped down its valley.* Water is
//!   conserved (`fluid`), and a reach standing a block over the next runs
//!   into it on the first tick until the upper one is dry. Holding it would
//!   need a weir of stone at every step, which is a different generator and
//!   a river nobody could row down.
//! * *A direction per river*, toward its mouth. A river here is a contour of
//!   a noise field and has no source and no mouth to point at; and a whole
//!   river flowing one way is a river that pours uphill somewhere along it.
//!
//! **What it does not know about is a player's own work.** The current is
//! the river's as it was generated: dam a river and the water above the dam
//! still pulls, dig a new channel and the water in it does not. The callers
//! only ask it about a body that is actually in water at the river's level,
//! so a dry bed carries nothing, and a canal is still water.

use super::scale::RiverOrder;
use super::{smoothstep, Preset, WorldGen, SEA_LEVEL};

/// How deep into its channel a column has to be before the water there
/// moves at all, and where it moves at full speed: `channel` is 1 on the
/// river's middle line and 0 where the cut meets the untouched bank, and the
/// water's edge is a little inside two thirds of the way in.
const SLACK_BANK: f64 = 0.35;
const FULL_STREAM: f64 = 0.85;

/// How steeply the country has to fall along a river for its water to run at
/// half its calm speed: very little. Almost every reach is running, and only
/// where the ground is level to a block in a few hundred does it stand.
const HALF_CALM_FALL: f64 = 0.004;

/// Where a reach starts to be a rapid, and where it is all rapid, as the fall
/// of the country along the channel: nine blocks in a hundred, and twenty.
///
/// **Measured against where the rivers are** (`how_fast_the_rivers_run`, two
/// seeds). A real river breaks white at a gradient of a few per cent, and
/// three and eight were tried first: three crossings in ten of a river were
/// rapids, and a brook's every tenth. The country a river runs through here
/// is hillier than the gradient of its own bed would be -- the fall is read
/// off the land it was cut through, hills and all -- so the line is higher
/// than a hydrologist's. At nine and twenty a river is a rapid at about one
/// crossing in four, where it comes down out of hills, a brook at one in
/// ten, and a great river never: the rest is lazy water a player swims.
const RAPID_FROM: f64 = 0.09;
const RAPID_TO: f64 = 0.2;

/// The speed a current has to reach before the water is drawn as broken
/// white water and a swimmer is not expected to hold against it, in blocks a
/// second: a little under what a swimmer makes (`physics::WATER_MOVE_FACTOR`
/// on a walking pace, about 1.6).
pub const RAPID_SPEED: f32 = 1.5;

/// How fast one order of river's water runs: calm, the speed of a reach the
/// country barely falls along; `rapid`, what a steep reach adds to it; and
/// `reach`, how far up and down the channel the country's fall is read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Flow {
    pub calm: f32,
    pub rapid: f32,
    pub reach: f64,
}

impl WorldGen {
    /// **Which way the river under this point is running**, in blocks a
    /// second as (x, z), and nothing at all anywhere that is not a
    /// generated river's channel at the river's level.
    ///
    /// `y` is where the body asking is: water above the sea's level is a
    /// lake or a pond, which stands still whatever channel it lies over.
    ///
    /// Continuous in `x` and `z` -- the four columns round the point are
    /// read and blended -- so a body crossing from one column to the next is
    /// not jolted, and a raft astride a line of columns is not torn.
    pub fn river_current(&self, x: f32, y: f32, z: f32) -> (f32, f32) {
        if self.preset == Preset::Test || !x.is_finite() || !z.is_finite() || !y.is_finite() {
            return (0.0, 0.0);
        }
        if y > SEA_LEVEL as f32 + 0.5 {
            return (0.0, 0.0);
        }
        // Column centres are at half blocks.
        let (u, v) = (f64::from(x) - 0.5, f64::from(z) - 0.5);
        let (i, k) = (u.floor(), v.floor());
        let (fu, fv) = ((u - i) as f32, (v - k) as f32);
        let (i, k) = (i as i32, k as i32);
        let at = |dx: i32, dz: i32| {
            let (px, pz) = self.on_planet(i.wrapping_add(dx), k.wrapping_add(dz));
            self.column_current(px, pz)
        };
        let (a, b, c, d) = (at(0, 0), at(1, 0), at(0, 1), at(1, 1));
        let blend = |a: f32, b: f32, c: f32, d: f32| {
            let near = a + (b - a) * fu;
            let far = c + (d - c) * fu;
            near + (far - near) * fv
        };
        (blend(a.0, b.0, c.0, d.0), blend(a.1, b.1, c.1, d.1))
    }

    /// The current at one column of the planet. See `river_current`.
    ///
    /// Walks the orders exactly as `terrain_height` cuts them, widest first
    /// and each on the ground the wider ones left, so a channel carries water
    /// exactly where it was cut.
    pub(super) fn column_current(&self, gx: i32, gz: i32) -> (f32, f32) {
        let (land, island) = self.land_before_rivers(gx, gz);
        if island {
            return (0.0, 0.0);
        }
        let mut height = land;
        let mut flow = (0.0f32, 0.0f32);
        for order in self.river_orders() {
            let cut = self.river(order, gx, gz, height);
            if cut.channel > SLACK_BANK {
                let (dx, dz) = self.order_current(order, gx, gz, smoothstep(SLACK_BANK, FULL_STREAM, cut.channel));
                flow.0 += dx;
                flow.1 += dz;
            }
            height = self.cut_order(order, gx, gz, height, &cut);
        }
        flow
    }

    /// One order's current at a column `share` of the way into full stream.
    fn order_current(&self, order: &RiverOrder, gx: i32, gz: i32, share: f64) -> (f32, f32) {
        let (x, z) = (f64::from(gx), f64::from(gz));
        // Along the channel: square to the field's slope, which points
        // across it. A central difference two blocks wide, finer than the
        // cut's own, because this is a direction a player swims against and
        // not a distance to a bank.
        const ACROSS: f64 = 2.0;
        let sx = self.river_field(order, x + ACROSS, z) - self.river_field(order, x - ACROSS, z);
        let sz = self.river_field(order, x, z + ACROSS) - self.river_field(order, x, z - ACROSS);
        let length = sx.hypot(sz);
        if length <= f64::EPSILON {
            return (0.0, 0.0);
        }
        let (tx, tz) = (-sz / length, sx / length);
        // How the country falls along it, over the order's reach either way.
        let reach = order.flow.reach;
        let ahead = self.land_before_rivers((x + tx * reach).round() as i32, (z + tz * reach).round() as i32).0;
        let behind = self.land_before_rivers((x - tx * reach).round() as i32, (z - tz * reach).round() as i32).0;
        let rise = (ahead - behind) / (2.0 * reach);
        let fall = rise.abs();
        let speed = f64::from(order.flow.calm) * fall / (fall + HALF_CALM_FALL)
            + f64::from(order.flow.rapid) * smoothstep(RAPID_FROM, RAPID_TO, fall);
        // Down the country: against the rise.
        let along = -rise.signum() * speed * share;
        ((tx * along) as f32, (tz * along) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;

    /// Columns in the water of a river's channel, found along straight walks
    /// the way `how_real_the_water_is` finds crossings, with the order they
    /// belong to: the middle column of every run of water under the sea's
    /// level that is a river and not the sea.
    fn channel_columns(gen: &WorldGen, walks: i32, length: i32) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for line in 0..walks {
            let along_x = line % 2 == 0;
            let offset = (line / 2 - walks / 4) * 3_001 + 211;
            let at = |i: i32| if along_x { (i, offset) } else { (offset, i) };
            let mut run = 0;
            for i in -length / 2..length / 2 {
                let (gx, gz) = at(i);
                if gen.height_at(gx, gz) < SEA_LEVEL {
                    run += 1;
                    continue;
                }
                if run > 0 {
                    let (mx, mz) = at(i - run / 2 - 1);
                    let h = gen.height_at(mx, mz);
                    if gen.biome_from(mx, mz, h) == Biome::River {
                        out.push((mx, mz));
                    }
                }
                run = 0;
            }
        }
        out
    }

    /// **A river runs downhill, along its channel.** In the middle of every
    /// channel crossed on a few walks, a current that is running at all runs
    /// along the channel and not across it, and toward the lower country.
    #[test]
    fn the_current_runs_along_a_river_and_down_the_country() {
        let gen = WorldGen::new(1337);
        let columns = channel_columns(&gen, 8, 12_000);
        let mut running = 0usize;
        for &(gx, gz) in &columns {
            let (vx, vz) = gen.river_current(gx as f32 + 0.5, (SEA_LEVEL - 1) as f32, gz as f32 + 0.5);
            let speed = vx.hypot(vz);
            if speed < 0.05 {
                continue;
            }
            running += 1;
            let (ux, uz) = (f64::from(vx / speed), f64::from(vz / speed));
            // Downhill: the country a river's reach downstream is lower than
            // the country as far upstream.
            let order = gen.river_orders().iter().min_by(|a, b| {
                let far = |o: &RiverOrder| gen.river_field(o, f64::from(gx), f64::from(gz)).abs() / o.frequency;
                far(a).total_cmp(&far(b))
            });
            let Some(order) = order else {
                continue;
            };
            // Along: square to the slope of the field the channel was cut on.
            let (x, z) = (f64::from(gx), f64::from(gz));
            let sx = gen.river_field(order, x + 2.0, z) - gen.river_field(order, x - 2.0, z);
            let sz = gen.river_field(order, x, z + 2.0) - gen.river_field(order, x, z - 2.0);
            let across = (ux * sx + uz * sz).abs() / sx.hypot(sz).max(f64::EPSILON);
            assert!(across < 0.35, "the river at {gx},{gz} runs {across:.2} of the way across its own channel");
            let reach = order.flow.reach;
            let land = |sign: f64| gen.land_before_rivers((f64::from(gx) + ux * reach * sign).round() as i32, (f64::from(gz) + uz * reach * sign).round() as i32).0;
            assert!(
                land(1.0) <= land(-1.0) + 0.5,
                "the river at {gx},{gz} runs uphill: {:.1} downstream against {:.1} up",
                land(1.0),
                land(-1.0)
            );
        }
        assert!(columns.len() >= 20, "only {} river crossings on the walks", columns.len());
        assert!(running * 2 >= columns.len(), "only {running} of {} river crossings are running", columns.len());
    }

    /// **The current has no seams.** A body moving a twentieth of a block
    /// anywhere in or beside a channel feels the current change by a small
    /// amount and never jump -- a jump is a swimmer jolted sideways crossing
    /// from one column to the next, and a raft astride two columns torn.
    #[test]
    fn the_current_changes_smoothly_from_place_to_place() {
        let gen = WorldGen::new(1337);
        let columns = channel_columns(&gen, 6, 10_000);
        let mut compared = 0usize;
        for &(gx, gz) in columns.iter().take(40) {
            for step in 0..200 {
                let x = gx as f32 - 5.0 + step as f32 * 0.05;
                for z in [gz as f32 + 0.3, gz as f32 + 0.8] {
                    let a = gen.river_current(x, 60.0, z);
                    let b = gen.river_current(x + 0.05, 60.0, z);
                    let jump = (a.0 - b.0).hypot(a.1 - b.1);
                    assert!(jump < 0.25, "the current jumps by {jump} over a twentieth of a block at {x},{z}");
                    compared += 1;
                }
            }
        }
        assert!(compared > 1_000, "only {compared} pairs compared");
    }

    /// **Standing water stands.** The sea (whose circulation is
    /// `fluid::current_at`'s), a lake above the sea's level, dry land, an
    /// island and the test world carry nothing.
    #[test]
    fn a_lake_the_sea_and_dry_land_carry_nothing() {
        let gen = WorldGen::new(1337);
        let columns = channel_columns(&gen, 6, 10_000);
        let &(gx, gz) = columns
            .iter()
            .find(|&&(gx, gz)| {
                let (vx, vz) = gen.river_current(gx as f32 + 0.5, 62.0, gz as f32 + 0.5);
                vx.hypot(vz) > 0.1
            })
            .expect("a running river on the walks");
        // The same column asked at a lake's level carries nothing.
        assert_eq!(gen.river_current(gx as f32 + 0.5, (SEA_LEVEL + 6) as f32, gz as f32 + 0.5), (0.0, 0.0));
        // The open sea, a long way out.
        let mut sea = None;
        for x in (0..400_000).step_by(1_000) {
            if gen.height_at(x, 0) < SEA_LEVEL - 30 {
                sea = Some(x);
                break;
            }
        }
        let sea = sea.expect("an ocean within 400 km");
        assert_eq!(gen.river_current(sea as f32 + 0.5, 50.0, 0.5), (0.0, 0.0));
        let test = WorldGen::with_preset(1337, Preset::Test);
        assert_eq!(test.river_current(gx as f32 + 0.5, 62.0, gz as f32 + 0.5), (0.0, 0.0));
    }

    /// **Most of a river is a swim, and some of it is a rapid.** Over the
    /// channels crossed on long walks, the river's middle runs slower than a
    /// swimmer across most of its length -- a river is crossable -- and
    /// faster somewhere, or there is no rapid and no decision.
    #[test]
    fn most_of_a_river_is_calm_and_some_of_it_is_a_rapid() {
        let gen = WorldGen::new(1337);
        let columns = channel_columns(&gen, 16, 20_000);
        let speeds: Vec<f32> = columns
            .iter()
            .map(|&(gx, gz)| {
                let (vx, vz) = gen.river_current(gx as f32 + 0.5, 62.0, gz as f32 + 0.5);
                vx.hypot(vz)
            })
            .collect();
        let rapid = speeds.iter().filter(|&&s| s >= RAPID_SPEED).count();
        let fastest = speeds.iter().copied().fold(0.0f32, f32::max);
        assert!(speeds.len() >= 40, "only {} crossings", speeds.len());
        assert!(rapid * 4 <= speeds.len(), "{rapid} of {} crossings are rapids", speeds.len());
        assert!(rapid >= 1, "not one rapid in {} crossings; the fastest ran at {fastest}", speeds.len());
        assert!(fastest < 4.5, "a river runs at {fastest} blocks a second");
    }

    /// Both sides of the wire work it out; neither sends it.
    #[test]
    fn the_same_place_is_the_same_current_on_every_generator_of_the_seed() {
        let a = WorldGen::new(1337);
        let columns = channel_columns(&a, 4, 8_000);
        let b = std::thread::spawn(|| WorldGen::new(1337)).join().expect("a generator");
        for &(gx, gz) in columns.iter().take(20) {
            let (x, z) = (gx as f32 + 0.37, gz as f32 + 0.61);
            assert_eq!(a.river_current(x, 61.0, z), b.river_current(x, 61.0, z));
        }
        assert_eq!(a.river_current(f32::NAN, 61.0, 0.0), (0.0, 0.0));
    }

    /// How fast the rivers run, by order: the share of crossings that are
    /// calm, running and rapid, and the fastest.
    ///
    /// ```text
    /// cargo test -p primitive_shared --lib -- --ignored --nocapture how_fast_the_rivers_run
    /// ```
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn how_fast_the_rivers_run() {
        for seed in [1337u32, 7] {
            let gen = WorldGen::new(seed);
            let columns = channel_columns(&gen, 16, 40_000);
            let mut speeds: Vec<f32> = columns
                .iter()
                .map(|&(gx, gz)| {
                    let (vx, vz) = gen.river_current(gx as f32 + 0.5, 62.0, gz as f32 + 0.5);
                    vx.hypot(vz)
                })
                .collect();
            // ...and by order: the order whose channel a crossing's middle is
            // deepest in.
            for (index, _) in gen.river_orders().iter().enumerate() {
                let mut mine: Vec<f32> = columns
                    .iter()
                    .filter(|&&(gx, gz)| {
                        let (land, _) = gen.land_before_rivers(gx, gz);
                        let mut height = land;
                        let mut best = (0usize, 0.0f64);
                        for (i, order) in gen.river_orders().iter().enumerate() {
                            let cut = gen.river(order, gx, gz, height);
                            if cut.channel > best.1 {
                                best = (i, cut.channel);
                            }
                            height = gen.cut_order(order, gx, gz, height, &cut);
                        }
                        best.0 == index
                    })
                    .map(|&(gx, gz)| {
                        let (vx, vz) = gen.river_current(gx as f32 + 0.5, 62.0, gz as f32 + 0.5);
                        vx.hypot(vz)
                    })
                    .collect();
                mine.sort_by(f32::total_cmp);
                let rapid = mine.iter().filter(|&&s| s >= RAPID_SPEED).count();
                println!(
                    "[current] seed {seed} order {index}: {} crossings, {rapid} rapids | p10 {:.2} p50 {:.2} p90 {:.2} max {:.2}",
                    mine.len(),
                    mine.get(mine.len() / 10).copied().unwrap_or(0.0),
                    mine.get(mine.len() / 2).copied().unwrap_or(0.0),
                    mine.get(mine.len() * 9 / 10).copied().unwrap_or(0.0),
                    mine.last().copied().unwrap_or(0.0)
                );
            }
            speeds.sort_by(f32::total_cmp);
            let share = |low: f32, high: f32| speeds.iter().filter(|&&s| s >= low && s < high).count() as f32 * 100.0 / speeds.len().max(1) as f32;
            let at = |p: f32| speeds.get(((speeds.len().max(1) - 1) as f32 * p) as usize).copied().unwrap_or(0.0);
            println!(
                "[current] seed {seed}: {} crossings | still (<0.1) {:.0}%, lazy (0.1-0.5) {:.0}%, running (0.5-{RAPID_SPEED}) {:.0}%, rapid {:.0}% | p50 {:.2} p90 {:.2} max {:.2}",
                speeds.len(),
                share(0.0, 0.1),
                share(0.1, 0.5),
                share(0.5, RAPID_SPEED),
                share(RAPID_SPEED, f32::MAX),
                at(0.5),
                at(0.9),
                at(1.0)
            );
        }
    }
}
