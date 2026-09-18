//! Этап 2: the client keeps only the chunks near the player, requests the
//! missing ones and drops the distant ones.
//!
//! Additions this pass:
//! - **Request retry.** The server now has bounded queues and *will* drop
//!   a chunk request under load. The old client asked for each chunk
//!   exactly once when it entered range, so a dropped request meant a
//!   permanent hole in the world you could fall through. Outstanding
//!   requests are now tracked with a timestamp and re-sent if the chunk
//!   hasn't arrived.
//! - **Neighbour access**, so the mesher can see across chunk boundaries
//!   for face culling and lighting.
//! - **A disc rather than a square.** See [`ChunkManager::inside`].
//!
//! Still purely local bookkeeping -- it doesn't talk to the network
//! itself; `main.rs` sends whatever `update()` hands back.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::engine::mesh::{LID_OPEN, LID_SWING_SECONDS};
use primitive_shared::lighting::BlockSource;
use primitive_shared::packed::PackedChunk;
use primitive_shared::types::{is_collidable, BlockId, ChunkPos, BLOCK_AIR, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

/// Whether a cell holds a body that is drawn as a figure. Bones are not:
/// they stay in the chunk mesh (`player_model::build_fallen`).
fn is_body(id: BlockId) -> bool {
    primitive_shared::types::block_kind(id) == primitive_shared::types::BLOCK_CORPSE
}

pub struct ChunkManager {
    /// Behind an `Arc` so a chunk can be handed to a worker thread
    /// without copying it.
    ///
    /// Lighting a chunk -- the expensive half of bringing it into the
    /// world -- runs off the main thread on its blocks. At the rate
    /// terrain streams, copying them for that was megabytes a second of
    /// pure memcpy. Sharing costs a reference count instead, and the one
    /// case where sharing is not free (an edit arriving while a worker
    /// still holds the chunk) is handled by `Arc::make_mut`, which copies
    /// exactly then and only then.
    ///
    /// **Packed, not flat.** A flat chunk became 131 KB when the world
    /// grew to 256 blocks, and two thirds of every one is sky: at render
    /// distance 24 that was 225 MB of block ids on the client alone. See
    /// `primitive_shared::packed` for the measurement and the form. The
    /// network thread packs a chunk before it ever reaches the frame (see
    /// `network::Incoming`), so nothing here pays for it.
    loaded: HashMap<ChunkPos, std::sync::Arc<PackedChunk>>,
    /// Chunks that have arrived from the server but haven't been
    /// integrated yet (integration is budgeted -- see `integrate_chunks`
    /// in main.rs).
    ///
    /// They must count as satisfied here, otherwise the retry logic
    /// keeps asking for them while they sit in the queue, the server
    /// dutifully sends them again, and the backlog feeds itself. That
    /// bug turned a 169-chunk area into 800+ queued arrivals.
    arrived: HashSet<ChunkPos>,
    /// Chunks we've asked for and haven't received yet, with the time of
    /// the last request.
    pending: HashMap<ChunkPos, Instant>,
    render_distance: i32,
    /// What [`ChunkManager::loaded_radius_blocks`] answers, worked out
    /// whenever the radius changes rather than once a frame.
    loaded_radius: f32,
    retry_after: Duration,
    last_scan: Option<Instant>,
    last_player_chunk: Option<ChunkPos>,
    pub requests_sent: u64,
    pub retries_sent: u64,
    /// Every dead player's body lying in a loaded chunk, by cell.
    ///
    /// **Kept here because a body is drawn outside the chunk mesh**, with the
    /// figure's own skin and clothes on the actor pipeline
    /// (`player_model::append_lying`), and the frame needs to know where they
    /// are without walking the world for them. Found as a chunk comes in
    /// (`PackedChunk::cells_where`, a palette lookup per section) and kept
    /// true by every edit that lands here, so it is never a second opinion
    /// about what is in the world.
    bodies: std::collections::BTreeSet<(i32, i32, i32)>,
    /// What each body is wearing, as the server last said
    /// (`ServerMessage::BodyWorn`). **Its own map, not part of `bodies`**,
    /// because the two arrive in either order: chunk integration is budgeted
    /// and the message is not, so the clothes can be here a frame before the
    /// chunk they lie in.
    body_worn: HashMap<(i32, i32, i32), [BlockId; primitive_shared::equipment::SLOTS]>,
    /// What is in each pit kiln, as the server last said
    /// (`ServerMessage::PitPottery`): `body_worn`'s arrangement, for its
    /// reason -- the message and the chunk arrive in either order. Read by
    /// the mesher through `pit_pottery_in`.
    pit_pottery: HashMap<(i32, i32, i32), Vec<BlockId>>,
    /// What lies in each cell a hand set something down in, as the server
    /// last said (`ServerMessage::SetDownItem`): `pit_pottery`'s arrangement,
    /// for its reason. Read by the frame, which draws each lying down
    /// (`entities::build_set_down_into`) -- not by the mesher, because the
    /// thing is an item model and the terrain pass cannot draw one.
    set_down: HashMap<(i32, i32, i32), BlockId>,
    /// Every chest whose lid is not lying shut: the ones somebody has open,
    /// and the ones still falling. Put here by `ServerMessage::ChestLid`,
    /// which is told to everybody who can see the chest and not only to
    /// whoever opened it, and read twice a frame -- by the frame, which
    /// swings the lid (`mesh::chest_lid_block`), and by the mesher, which
    /// leaves it out of the chunk while the frame has it (`lids_in`).
    lids: HashMap<(i32, i32, i32), Lid>,
    /// A lid changed hands between the mesh and the frame since the frame
    /// last asked. See `take_lid_handover`.
    lid_handover: bool,
}

/// A chest lid on its way up, standing open, or on its way down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lid {
    /// Where it is heading: open while anybody has the chest open.
    pub open: bool,
    /// How far it has turned, in radians, 0 shut.
    pub swing: f32,
    /// Whether the chunk mesh *on screen* leaves this lid out -- which is the
    /// one thing that decides whether the frame draws it. See
    /// [`ChunkManager::note_meshed_lids`].
    pub meshed_out: bool,
}

impl Lid {
    /// Whether the mesh being built now should leave the lid out: while it
    /// is open or still moving. A lid at rest shut belongs to the mesh.
    fn wants_out(&self) -> bool {
        self.open || self.swing > 0.0
    }
}

/// One dead player's body, as the frame draws it: where it lies and what it
/// has on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub cell: (i32, i32, i32),
    pub worn: [BlockId; primitive_shared::equipment::SLOTS],
}

/// A column of cells at one (x, z), already resolved to its chunk.
///
/// Borrowed rather than copied: it is a pointer and two indices, and it
/// exists only so a caller reading several cells of one column pays for
/// finding the chunk once. See `ChunkManager::column`.
pub struct Column<'a> {
    chunk: &'a PackedChunk,
    lx: usize,
    lz: usize,
}

impl Column<'_> {
    /// The block at this height. Outside the world is air, which is what
    /// the single-cell lookup answers too -- there is nothing to stand
    /// on above the sky or below the bedrock.
    #[inline]
    pub fn block(&self, gy: i32) -> BlockId {
        if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
            return BLOCK_AIR;
        }
        self.chunk.get(self.lx, gy as usize, self.lz)
    }
}

/// The eight neighbours, in a fixed order.
pub const NEIGHBOUR_OFFSETS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

impl ChunkManager {
    pub fn new(render_distance: i32) -> Self {
        Self {
            loaded: HashMap::new(),
            arrived: HashSet::new(),
            pending: HashMap::new(),
            render_distance: render_distance.max(1),
            loaded_radius: Self::reach_of(render_distance.max(1)),
            retry_after: Duration::from_secs(3),
            last_scan: None,
            last_player_chunk: None,
            requests_sent: 0,
            retries_sent: 0,
            bodies: std::collections::BTreeSet::new(),
            body_worn: HashMap::new(),
            pit_pottery: HashMap::new(),
            set_down: HashMap::new(),
            lids: HashMap::new(),
            lid_handover: false,
        }
    }

    /// What the server says about the chest at this cell: somebody has it
    /// open, or nobody has any more.
    ///
    /// **Returns whether the chunk has to be meshed again**, which is exactly
    /// when the lid changes hands. It leaves the chunk mesh the moment this
    /// hears of it and comes back when it has finished falling
    /// (`advance_lids`) -- never in between, however many players open and
    /// shut the same chest while it is up.
    pub fn note_chest_lid(&mut self, cell: (i32, i32, i32), open: bool) -> bool {
        match self.lids.get_mut(&cell) {
            Some(lid) => {
                // A lid at rest shut and on its way back into the mesh that
                // is opened again has to be taken out of the next one.
                let was = lid.wants_out();
                lid.open = open;
                lid.wants_out() != was
            }
            // A chest that is told it is shut and has no lid up is the
            // ordinary case of hearing twice: nothing to swing, nothing to
            // mesh.
            None if !open => false,
            None => {
                self.lids.insert(cell, Lid { open, swing: 0.0, meshed_out: false });
                true
            }
        }
    }

    /// **A chunk mesh has just gone on screen**, built leaving out the lids in
    /// `left_out` (`Neighbourhood::set_swung_lids`, carried back with the
    /// result). This is the moment the lid changes hands, and not the moment
    /// the mesh was asked for.
    ///
    /// **Why: "сундук мерцает при анимации".** The frame took the lid the
    /// moment the server said open, and handed it back the moment it came to
    /// rest -- and a chunk mesh lands frames after it is asked for. So on
    /// the way up the chunk still drew the lid shut *and* the frame drew it
    /// rising out of the same place, two lids fighting through each other for
    /// the first degrees of the swing; and on the way down there were frames
    /// with no lid at all, between the frame letting go and the mesh taking
    /// it. Now the frame draws a lid exactly while the mesh on screen does
    /// not: the swing waits at shut until the lidless mesh lands, and a lid
    /// come to rest is drawn by the frame, shut, until the mesh with it does.
    ///
    /// Whether anything changed hands is kept for the frame, which has to
    /// rebuild its moving geometry on that frame -- see `take_lid_handover`.
    pub fn note_meshed_lids(&mut self, pos: ChunkPos, left_out: &[(i32, i32, i32)]) {
        let (x0, z0) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
        let inside =
            |&(x, _, z): &(i32, i32, i32)| x >= x0 && x < x0 + CHUNK_SIZE_X as i32 && z >= z0 && z < z0 + CHUNK_SIZE_Z as i32;
        let mut changed = false;
        self.lids.retain(|cell, lid| {
            if !inside(cell) {
                return true;
            }
            let out = left_out.contains(cell);
            changed |= out != lid.meshed_out;
            lid.meshed_out = out;
            // Back in the mesh and nothing left to swing: the frame is done.
            out || lid.wants_out()
        });
        self.lid_handover |= changed;
    }

    /// Whether a lid changed hands since this was last asked, so the frame
    /// draws its side of the handover on the frame the mesh does -- the
    /// moving geometry otherwise waits for its own clock
    /// (`dynamic_rebuild_due`), and a lid left out of a mesh a frame before
    /// the frame drew it was the other half of the flicker.
    pub fn take_lid_handover(&mut self) -> bool {
        std::mem::take(&mut self.lid_handover)
    }

    /// Moves every lid `seconds` further along, and gives back the cells of
    /// the chests whose lids have just come to rest shut -- the chunks that
    /// have to be meshed again to take the lid back (`note_chest_lid`).
    ///
    /// **A rate and not a curve.** A lid that eased in and out would want the
    /// swing to know when it started, and the swing is told "open" and "shut"
    /// by a server that may say both within one frame; a constant rate has no
    /// state beyond the angle itself and cannot be caught halfway by a
    /// message. What it costs is the ease, on a piece of wood on an iron
    /// hinge, which is not a thing that eases.
    pub fn advance_lids(&mut self, seconds: f32) -> Vec<(i32, i32, i32)> {
        let step = seconds * LID_OPEN / LID_SWING_SECONDS;
        let mut rested = Vec::new();
        for (&cell, lid) in self.lids.iter_mut() {
            // Held shut until the mesh on screen has let go of it: a lid the
            // frame is not drawing does not move. See `note_meshed_lids`.
            if !lid.meshed_out {
                continue;
            }
            let was = lid.wants_out();
            lid.swing = if lid.open {
                (lid.swing + step).min(LID_OPEN)
            } else {
                (lid.swing - step).max(0.0)
            };
            if was && !lid.wants_out() {
                rested.push(cell);
            }
        }
        rested
    }

    /// Every chest lid the frame is drawing: its cell, the block there (which
    /// way the chest was put down, and what it is made of) and how far it has
    /// swung.
    ///
    /// **Only where the cell is still a chest**, for `set_down_items`' reason:
    /// the message and the chunk arrive in either order, and a lid swinging
    /// over the hole where somebody broke the chest is a lid in the air.
    pub fn open_lids(&self) -> impl Iterator<Item = ((i32, i32, i32), BlockId, f32)> + '_ {
        self.lids.iter().filter(|(_, lid)| lid.meshed_out).filter_map(|(&cell, lid)| {
            let block = self.block_at(cell.0, cell.1, cell.2)?;
            (primitive_shared::types::block_kind(block) == primitive_shared::types::BLOCK_CHEST)
                .then_some((cell, block, lid.swing))
        })
    }

    /// The cells in a chunk whose lid the frame is drawing, for the mesher
    /// (`Neighbourhood::set_swung_lids`). Nearly always empty, and then it
    /// allocates nothing.
    pub fn lids_in(&self, pos: ChunkPos) -> Vec<(i32, i32, i32)> {
        if self.lids.is_empty() {
            return Vec::new();
        }
        let (x0, z0) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
        self.lids
            .iter()
            .filter(|(_, lid)| lid.wants_out())
            .map(|(&cell, _)| cell)
            .filter(|&(x, _, z)| x >= x0 && x < x0 + CHUNK_SIZE_X as i32 && z >= z0 && z < z0 + CHUNK_SIZE_Z as i32)
            .collect()
    }

    /// What the server says lies in the set-down cell at `cell`; air for
    /// nothing.
    pub fn note_set_down(&mut self, cell: (i32, i32, i32), item: BlockId) {
        if item == BLOCK_AIR {
            self.set_down.remove(&cell);
        } else {
            self.set_down.insert(cell, item);
        }
    }

    /// Every thing set down in the loaded world, with its cell and the id of
    /// the cell (which way it was turned).
    ///
    /// **Only where the cell still says so.** The message and the chunk
    /// arrive in either order, and a thing heard about in a chunk not yet
    /// integrated -- or in a cell a prediction has just emptied -- is not
    /// drawn floating over whatever the grid holds there.
    pub fn set_down_items(&self) -> impl Iterator<Item = ((i32, i32, i32), BlockId, BlockId)> + '_ {
        self.set_down.iter().filter_map(|(&cell, &item)| {
            let block = self.block_at(cell.0, cell.1, cell.2)?;
            primitive_shared::types::is_set_down(block).then_some((cell, block, item))
        })
    }

    /// What the server says is in the pit kiln at this cell.
    pub fn note_pit_pottery(&mut self, cell: (i32, i32, i32), pieces: Vec<BlockId>) {
        if pieces.is_empty() {
            self.pit_pottery.remove(&cell);
        } else {
            self.pit_pottery.insert(cell, pieces);
        }
    }

    /// Every pit kiln in a chunk the server has said the contents of, for
    /// the mesher (`Neighbourhood::set_pottery`). Nearly always empty, and
    /// then it allocates nothing.
    pub fn pit_pottery_in(&self, pos: ChunkPos) -> Vec<((i32, i32, i32), Vec<BlockId>)> {
        if self.pit_pottery.is_empty() {
            return Vec::new();
        }
        let (x0, z0) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
        self.pit_pottery
            .iter()
            .filter(|(&(x, _, z), _)| {
                x >= x0 && x < x0 + CHUNK_SIZE_X as i32 && z >= z0 && z < z0 + CHUNK_SIZE_Z as i32
            })
            .map(|(&cell, pieces)| (cell, pieces.clone()))
            .collect()
    }

    /// Every body lying in the loaded world, with what it is wearing -- bare
    /// until the server has said otherwise.
    pub fn bodies(&self) -> impl Iterator<Item = Body> + '_ {
        self.bodies.iter().map(|&cell| Body {
            cell,
            worn: self.body_worn.get(&cell).copied().unwrap_or([BLOCK_AIR; primitive_shared::equipment::SLOTS]),
        })
    }

    /// Whether a body lies in this column between these heights, inclusive.
    ///
    /// What decides whether a dead player is drawn at all: the body a death
    /// leaves *is* them, in their column a little above or below their feet
    /// (`primitive_server::corpse_cell`), and a figure drawn beside it would
    /// be the same person lying down twice.
    pub fn body_in_column(&self, x: i32, z: i32, low: i32, high: i32) -> bool {
        self.bodies.range((x, low, z)..=(x, high, z)).any(|&(bx, _, bz)| bx == x && bz == z)
    }

    /// What the server says the body at this cell has on. See `body_worn`.
    pub fn note_body_worn(&mut self, cell: (i32, i32, i32), worn: [BlockId; primitive_shared::equipment::SLOTS]) {
        self.body_worn.insert(cell, worn);
    }

    /// Forgets every body in a chunk, and what they wore when `worn` is set.
    fn forget_bodies(&mut self, pos: ChunkPos, worn: bool) {
        let (x0, z0) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
        let inside = |&(x, _, z): &(i32, i32, i32)| {
            !(x >= x0 && x < x0 + CHUNK_SIZE_X as i32 && z >= z0 && z < z0 + CHUNK_SIZE_Z as i32)
        };
        self.bodies.retain(inside);
        if worn {
            self.body_worn.retain(|cell, _| inside(cell));
            // ...and what was in its pits, on the same terms.
            self.pit_pottery.retain(|cell, _| inside(cell));
            // ...and what was set down in it.
            self.set_down.retain(|cell, _| inside(cell));
            // ...and any lid it had up. The server says it again behind the
            // chunk when the chunk comes back (`ServerMessage::ChestLid`,
            // sent by `open_lids_in`), so a chest somebody is still standing
            // at comes back open rather than shut.
            self.lids.retain(|cell, _| inside(cell));
        }
    }

    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// Bytes the loaded chunks' blocks keep on the heap -- the cells, not
    /// the map. For the F3 line: a walk of sixteen headers a chunk, once
    /// a second, so that what `packed` saves can be read off a running
    /// game rather than taken on trust.
    pub fn heap_bytes(&self) -> usize {
        self.loaded.values().map(|chunk| chunk.heap_bytes()).sum()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn render_distance(&self) -> i32 {
        self.render_distance
    }

    /// Is a chunk `dx`, `dz` chunks away one of the ones we keep?
    ///
    /// **A disc, not a square**, and the reason is what the player can
    /// actually see. The streamed area used to be the square
    /// `-r..=r` on both axes, which reaches `r` chunks along the axes
    /// and `r * sqrt(2)` into the corners -- forty per cent further in
    /// four directions, for no reason anybody chose.
    ///
    /// Those corners are the chunks the fog was already hiding. Fog is a
    /// *distance*, so it fades out in a circle round the player (see
    /// `engine::fog`), and everything outside that circle is generated,
    /// sent over the socket, lit, meshed and drawn in order to be
    /// invisible. Cutting the square down to the circle it contains is
    /// therefore free where it matters and cheap where it does not:
    ///
    /// | radius | square | disc | saved |
    /// |---|---|---|---|
    /// | 4 | 81 | 49 | 40% |
    /// | 8 | 289 | 197 | 32% |
    /// | 16 | 1089 | 797 | 27% |
    ///
    /// It also makes the world end at the same distance in every
    /// direction, which is what the fog is drawn against
    /// (`frame_params` hands the renderer `render_distance * 16` as a
    /// single number). A square horizon behind a circular fade is the
    /// one arrangement that can show the edge of the world: turn
    /// forty-five degrees and the terrain reaches further than the fade
    /// that was meant to cover it.
    ///
    /// Squared integers on both sides -- no square root, no float, and
    /// the same answer on every machine.
    ///
    /// **The nine chunks around the player are in whatever the radius
    /// says.** They are not a matter of distance: physics stands on
    /// them, `is_area_ready` waits for all nine before letting the
    /// player move, and at a render distance of 1 a strict disc leaves
    /// out the four diagonals -- so the loading gate would wait for
    /// chunks the streamer had decided never to ask for, forever. Above
    /// a radius of two the disc contains them anyway and this costs
    /// nothing.
    #[inline]
    pub fn inside(&self, dx: i32, dz: i32) -> bool {
        if dx.abs() <= 1 && dz.abs() <= 1 {
            return true;
        }
        dx * dx + dz * dz <= self.render_distance * self.render_distance
    }

    /// How far the streamed disc reaches, in blocks, in the worst
    /// direction from the worst place to be standing.
    ///
    /// **What the fog is clamped to** -- see `Fog::clamp_to`. A disc
    /// reaches `r` chunks along the axes and, because a chunk is a
    /// square and the test is on chunk *indices*, rather less than that
    /// off them: at a radius of eight the nearest cell nobody streams is
    /// 107 blocks away, against the 128 the axes reach. Fog that faded
    /// out at 128 would therefore have a sliver of open sky under it,
    /// which is the one thing a fade exists to prevent.
    ///
    /// Measured from the corner of the middle chunk *nearest the gap*,
    /// because that is where a player can actually stand, and to the
    /// near corner of the first chunk that is not streamed, because that
    /// is where the world ends. Scanned rather than reasoned out: the
    /// worst direction is **not** the forty-five degree one, which is
    /// the answer a person writes down first. At a radius of eight it is
    /// the chunk at (7, 4) -- outside the circle by one, and with only
    /// six chunks and three of clearance in front of it.
    pub fn loaded_radius_blocks(&self) -> f32 {
        self.loaded_radius
    }

    /// `loaded_radius_blocks` for a radius, without a manager to ask.
    ///
    /// For the fog, which is laid out against where the world ends
    /// (`ClientSettings::fog_range`) and is built in places that hold no
    /// manager: the frame before a world has streamed, the offscreen tools,
    /// the settings tests.
    pub fn reach_blocks(render_distance: i32) -> f32 {
        Self::reach_of(render_distance.max(1))
    }

    /// Works that out. Cheap -- a square of `r + 2` a side, once per
    /// change of radius -- and cached, because the fog asks every frame.
    fn reach_of(render_distance: i32) -> f32 {
        let mut nearest = f32::MAX;
        for dx in 0..=render_distance + 1 {
            for dz in 0..=render_distance + 1 {
                let streamed = (dx.abs() <= 1 && dz.abs() <= 1)
                    || dx * dx + dz * dz <= render_distance * render_distance;
                if streamed {
                    continue;
                }
                // The gap between the two chunks, in chunks: a cell one
                // step away shares an edge with the middle one, so
                // nothing separates them at all.
                let gap_x = (dx - 1).max(0) as f32;
                let gap_z = (dz - 1).max(0) as f32;
                nearest = nearest.min(gap_x.hypot(gap_z));
            }
        }
        if !nearest.is_finite() {
            // Nothing is excluded, which cannot happen for a finite
            // radius -- but a fog range of infinity would be a NaN
            // waiting to happen in the shader.
            nearest = render_distance as f32;
        }
        nearest * primitive_shared::types::CHUNK_SIZE_X as f32
    }

    /// Changes the radius without discarding what is already loaded.
    ///
    /// Rebuilding the manager instead would drop every chunk and every
    /// mesh and stream the world back in from nothing, which is a
    /// second of empty sky for a setting the player expects to take
    /// effect quietly. Growing it lets the next `update` ask for the new
    /// ring; shrinking it lets the same call unload the outer one.
    pub fn set_render_distance(&mut self, render_distance: i32) {
        let wanted = render_distance.max(1);
        if wanted == self.render_distance {
            return;
        }
        self.render_distance = wanted;
        self.loaded_radius = Self::reach_of(wanted);
        // Force the next `update` to rescan. It normally skips unless
        // the player crossed a chunk boundary, and standing still while
        // turning the setting up is exactly the case that matters.
        self.last_player_chunk = None;
    }

    /// How much of the immediate 3x3 neighbourhood around `centre` is
    /// loaded, as (loaded, needed).
    ///
    /// This is what the loading screen reports and what gates physics.
    /// Only the 3x3 matters, not the whole render distance: the player
    /// can only fall into or walk into a chunk they're touching, so
    /// waiting for the entire ring would keep them staring at a loading
    /// bar long after the world under their feet was solid.
    pub fn spawn_area_progress(&self, centre: ChunkPos) -> (usize, usize) {
        let mut loaded = 0;
        let mut needed = 0;
        for dx in -1..=1 {
            for dz in -1..=1 {
                needed += 1;
                if self.loaded.contains_key(&ChunkPos::new(centre.x + dx, centre.z + dz)) {
                    loaded += 1;
                }
            }
        }
        (loaded, needed)
    }

    /// Is this chunk's neighbourhood settled enough to mesh?
    ///
    /// A chunk's mesh depends on its eight neighbours (face culling and
    /// lighting across the seam), so meshing it before they arrive means
    /// meshing it again for each one that shows up. During a fresh
    /// stream of 169 chunks that turned ~169 mesh jobs into well over a
    /// thousand -- the reason the frame rate sagged while terrain
    /// loaded.
    ///
    /// A neighbour counts as settled if it's loaded *or* if it lies
    /// outside the streamed area and is therefore never coming. That
    /// second case matters: without it the outermost ring would wait
    /// forever and never render.
    ///
    /// **The same test [`ChunkManager::inside`] uses**, and it has to be:
    /// this decides "never coming" and `update` decides what is asked
    /// for, and a chunk that one of them thinks is outside while the
    /// other thinks is inside is either a permanently unmeshed ring or a
    /// mesh built against terrain that has not arrived. When the
    /// streamed area was a square and this was a chebyshev distance they
    /// agreed by accident; now they agree by construction.
    pub fn neighbourhood_settled(&self, pos: ChunkPos, centre: ChunkPos) -> bool {
        for (dx, dz) in NEIGHBOUR_OFFSETS {
            let neighbour = ChunkPos::new(pos.x + dx, pos.z + dz);
            if self.loaded.contains_key(&neighbour) {
                continue;
            }
            if !self.inside(neighbour.x - centre.x, neighbour.z - centre.z) {
                continue; // outside the streamed area; it will never arrive
            }
            return false;
        }
        true
    }

    /// True once the ground under and around the player exists.
    ///
    /// Asked on entering a world and again after every teleport: an
    /// unloaded chunk reads as air, so simulating a player standing in
    /// one drops them through the world. See `main::respawn_gate`.
    pub fn is_area_ready(&self, centre: ChunkPos) -> bool {
        let (loaded, needed) = self.spawn_area_progress(centre);
        loaded == needed
    }

    pub fn is_loaded(&self, pos: ChunkPos) -> bool {
        self.loaded.contains_key(&pos)
    }

    pub fn chunk_for_world_pos(world_x: impl Into<f64>, world_z: impl Into<f64>) -> ChunkPos {
        ChunkPos::from_world(world_x, world_z)
    }

    /// Called every frame. Returns (chunks to request, chunks to unload).
    ///
    /// The expensive part -- building the wanted set -- only runs when the
    /// player crosses a chunk boundary or when the retry timer is due,
    /// not 60 times a second.
    pub fn update(
        &mut self,
        player_chunk: ChunkPos,
        now: Instant,
    ) -> (Vec<ChunkPos>, Vec<ChunkPos>) {
        let moved = self.last_player_chunk != Some(player_chunk);
        let due = self
            .last_scan
            .map(|t| now.duration_since(t) >= Duration::from_millis(500))
            .unwrap_or(true);
        if !moved && !due {
            return (Vec::new(), Vec::new());
        }
        self.last_player_chunk = Some(player_chunk);
        self.last_scan = Some(now);

        // The square is the *loop*; the disc is what comes out of it.
        // Walking the bounding box and testing each cell is the cheap
        // way round: the alternative -- working out the run of `dx` for
        // each `dz` -- is the same set of chunks and one more thing to
        // get wrong at the edges.
        let mut wanted = HashSet::new();
        for dx in -self.render_distance..=self.render_distance {
            for dz in -self.render_distance..=self.render_distance {
                if !self.inside(dx, dz) {
                    continue;
                }
                wanted.insert(ChunkPos::new(player_chunk.x + dx, player_chunk.z + dz));
            }
        }

        let mut to_request = Vec::new();
        for pos in &wanted {
            if self.loaded.contains_key(pos) || self.arrived.contains(pos) {
                continue;
            }
            match self.pending.get(pos) {
                Some(&requested_at) if now.duration_since(requested_at) < self.retry_after => {}
                Some(_) => {
                    self.retries_sent += 1;
                    self.pending.insert(*pos, now);
                    to_request.push(*pos);
                }
                None => {
                    self.requests_sent += 1;
                    self.pending.insert(*pos, now);
                    to_request.push(*pos);
                }
            }
        }

        // **Nearest first, and this is the whole of why a world takes a
        // moment to open rather than a quarter of a minute.**
        //
        // `wanted` is a `HashSet`, so iterating it hands out the render
        // distance in hash order -- which is to say shuffled. The
        // server sends what it was asked for in the order it was asked
        // (see `connection`'s chunk pump) at a bounded rate, so the
        // ground under the player's feet was somewhere in the middle of
        // eighteen hundred requests. The client will not let anybody
        // move until the three-by-three around them has arrived, and
        // waiting for the last of nine uniformly scattered positions
        // took **12.8 seconds** measured, on a world that streams two
        // thousand chunks at a hundred and sixty a second.
        //
        // Sorted, those nine are the first nine sent. Everything else
        // is unchanged -- the same chunks, the same rate, the same
        // total time to fill the horizon -- but the player is standing
        // on solid ground a fifth of a second in and the world grows
        // outward around them, which is also what it should look like.
        //
        // Eighteen hundred keys once every half second, against a
        // saving of twelve seconds.
        to_request.sort_unstable_by_key(|pos| {
            let dx = (pos.x - player_chunk.x) as i64;
            let dz = (pos.z - player_chunk.z) as i64;
            dx * dx + dz * dz
        });

        let to_unload: Vec<ChunkPos> = self
            .loaded
            .keys()
            .filter(|pos| !wanted.contains(pos))
            .copied()
            .collect();

        // Stop chasing chunks that are no longer wanted.
        self.pending.retain(|pos, _| wanted.contains(pos));

        (to_request, to_unload)
    }

    /// Call the moment a chunk arrives, before it's integrated, so it
    /// stops being re-requested.
    ///
    /// (test note: see `the_ground_underfoot_is_asked_for_first`.)
    pub fn note_arrival(&mut self, pos: ChunkPos) {
        self.pending.remove(&pos);
        self.arrived.insert(pos);
    }

    /// Takes a chunk in, and hands back the shared copy.
    ///
    /// Returning it is what lets the caller pass the same blocks to a
    /// lighting worker without a second copy of them.
    ///
    /// Takes anything that packs, so a flat `Chunk` from a test fixture
    /// and an already packed one from the network both go in the same
    /// way -- and a packed one is moved, not packed again.
    pub fn insert(&mut self, chunk: impl Into<PackedChunk>) -> std::sync::Arc<PackedChunk> {
        let chunk = chunk.into();
        self.arrived.remove(&chunk.pos);
        self.pending.remove(&chunk.pos);
        let pos = chunk.pos;
        // The clothes are kept: they may have come first (see `body_worn`).
        self.forget_bodies(pos, false);
        let (x0, z0) = (pos.x * CHUNK_SIZE_X as i32, pos.z * CHUNK_SIZE_Z as i32);
        let mut found = Vec::new();
        chunk.cells_where(is_body, |x, y, z| found.push((x0 + x as i32, y as i32, z0 + z as i32)));
        self.bodies.extend(found);
        let shared = std::sync::Arc::new(chunk);
        self.loaded.insert(pos, std::sync::Arc::clone(&shared));
        shared
    }

    pub fn unload(&mut self, pos: ChunkPos) {
        // ...and here they go with it: the server sends them again behind
        // the chunk when it is asked for again.
        self.forget_bodies(pos, true);
        self.loaded.remove(&pos);
        self.pending.remove(&pos);
        self.arrived.remove(&pos);
    }

    pub fn get(&self, pos: ChunkPos) -> Option<&PackedChunk> {
        self.loaded.get(&pos).map(|chunk| &**chunk)
    }

    /// Neighbour chunk lookup for the mesher's padded volume.
    #[allow(dead_code)] // kept: the natural neighbour accessor, used by tests
    pub fn neighbour(&self, pos: ChunkPos, dx: i32, dz: i32) -> Option<&PackedChunk> {
        self.loaded
            .get(&ChunkPos::new(pos.x + dx, pos.z + dz))
            .map(|chunk| &**chunk)
    }

    #[allow(dead_code)] // used by tests; kept as the natural iteration API
    pub fn iter(&self) -> impl Iterator<Item = &PackedChunk> {
        self.loaded.values().map(|chunk| &**chunk)
    }

    /// Block at a global (block-space) coordinate. `None` means "we don't
    /// have that chunk loaded, so we don't actually know" -- physics
    /// treats that as non-solid, which means you *can* fall through the
    /// world at the edge of loaded chunks. Acceptable for a prototype;
    /// the request-retry above at least means the hole gets filled.
    pub fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
        if gy < 0 || gy as usize >= CHUNK_SIZE_Y {
            return Some(BLOCK_AIR);
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        self.loaded.get(&pos).map(|c| c.get(lx, gy as usize, lz))
    }

    /// Solid for collision purposes. Note this asks `is_collidable`, not
    /// "is not air": water is a block you can be inside, and treating it
    /// as solid is what used to let players walk across lakes.
    #[allow(dead_code)] // the natural single-cell query; physics reads columns
    pub fn is_solid(&self, gx: i32, gy: i32, gz: i32) -> bool {
        matches!(self.block_at(gx, gy, gz), Some(id) if is_collidable(id))
    }

    /// One vertical column of the world, found once and then read
    /// without further lookups.
    ///
    /// Chunks live in a hash map, so every `block_at` costs a hash of
    /// the chunk position -- and everything that walks a *volume* of
    /// cells (collision, which spans four columns and three cells of
    /// height, several times a frame) pays that hash per cell for an
    /// answer that is the same chunk every time. The same
    /// once-per-column rather than once-per-cell move that took chunk
    /// integration from 456 ms/s to 2 (see the mesher's
    /// `Neighbourhood`), applied to the other side of the client.
    pub fn column(&self, gx: i32, gz: i32) -> Option<Column<'_>> {
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        Some(Column {
            chunk: self.loaded.get(&pos)?,
            lx,
            lz,
        })
    }

    /// What a build/break ray can hit -- see `types::is_targetable` for
    /// which blocks those are and why grass used to be missing from
    /// them.
    ///
    /// The raycast no longer asks: it needs the block itself, because
    /// whether the ray hits depends on how much of the cell the block
    /// fills. Kept as the plain question, which is still the right one
    /// for anything asking about a cell rather than about a ray.
    #[allow(dead_code)]
    pub fn is_targetable(&self, gx: i32, gy: i32, gz: i32) -> bool {
        matches!(self.block_at(gx, gy, gz), Some(id) if primitive_shared::types::is_targetable(id))
    }

    /// Applies a confirmed block edit from the server. Returns the chunk's
    /// position if something actually changed, so the caller knows which
    /// mesh to rebuild.
    pub fn apply_block_update(
        &mut self,
        gx: i32,
        gy: i32,
        gz: i32,
        block_id: BlockId,
    ) -> Option<ChunkPos> {
        if gy < 0 || gy as usize >= CHUNK_SIZE_Y {
            return None;
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        let shared = self.loaded.get_mut(&pos)?;
        if shared.get(lx, gy as usize, lz) == block_id {
            return None; // nothing changed; don't force a re-mesh
        }
        // Copies only if a lighting worker is still holding this chunk,
        // which is a window of a few milliseconds after it arrives.
        std::sync::Arc::make_mut(shared).set(lx, gy as usize, lz, block_id);
        if is_body(block_id) {
            self.bodies.insert((gx, gy, gz));
        } else if self.bodies.remove(&(gx, gy, gz)) {
            // Rotted to bones or broken open: whatever it wore is not lying
            // there any more.
            self.body_worn.remove(&(gx, gy, gz));
        }
        // A pit broken open or emptied is not holding pottery.
        if !primitive_shared::pit::is_pit_kiln(block_id) {
            self.pit_pottery.remove(&(gx, gy, gz));
        }
        // ...and a chest that has gone has no lid up. Nothing tells the
        // client otherwise: the lid is shut for everyone at the chest by the
        // break itself (`close_chest_for_everyone`), and by then there is no
        // chest there to send a message about. Left behind, the cell would
        // still be on the mesher's list of lids to leave out -- so the next
        // chest put down in it would be built without one.
        if primitive_shared::types::block_kind(block_id) != primitive_shared::types::BLOCK_CHEST {
            self.lids.remove(&(gx, gy, gz));
        }
        // ...and a thing taken back is not lying there.
        if !primitive_shared::types::is_set_down(block_id) {
            self.set_down.remove(&(gx, gy, gz));
        }
        Some(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::Chunk;

    /// **The bodies the frame draws are the bodies in the world**: found in
    /// a chunk as it arrives, added by the edit that lays one, dropped by the
    /// edit that rots it to bones -- with its clothes -- and by the chunk
    /// going out of range. Clothes heard before the chunk arrived are kept.
    #[test]
    fn the_bodies_drawn_are_the_bodies_lying_in_the_loaded_world() {
        use primitive_shared::types::{BLOCK_CORPSE, BLOCK_IRON_HELM, BLOCK_REMAINS, CHUNK_VOLUME};
        let mut chunks = ChunkManager::new(4);
        let mut helm = [BLOCK_AIR; primitive_shared::equipment::SLOTS];
        helm[0] = BLOCK_IRON_HELM;
        chunks.note_body_worn((19, 40, 3), helm);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        blocks[Chunk::index(3, 40, 3)] = BLOCK_CORPSE;
        chunks.insert(Chunk { pos: ChunkPos::new(1, 0), blocks });
        let bodies: Vec<Body> = chunks.bodies().collect();
        assert_eq!(bodies, vec![Body { cell: (19, 40, 3), worn: helm }]);
        assert!(chunks.body_in_column(19, 3, 38, 43) && !chunks.body_in_column(19, 4, 0, 255));

        chunks.apply_block_update(20, 41, 5, BLOCK_CORPSE);
        assert_eq!(chunks.bodies().count(), 2);
        chunks.apply_block_update(19, 40, 3, BLOCK_REMAINS);
        assert_eq!(chunks.bodies().map(|b| b.cell).collect::<Vec<_>>(), vec![(20, 41, 5)]);
        chunks.apply_block_update(19, 40, 3, BLOCK_CORPSE);
        assert_eq!(chunks.bodies().find(|b| b.cell == (19, 40, 3)).map(|b| b.worn), Some([BLOCK_AIR; primitive_shared::equipment::SLOTS]), "the bones kept the helmet");
        chunks.unload(ChunkPos::new(1, 0));
        assert_eq!(chunks.bodies().count(), 0);
    }

    /// **A lid goes up while somebody is at the chest and comes down after**,
    /// and the chunk is meshed again exactly twice: once when the frame takes
    /// the lid and once when it gives it back.
    ///
    /// The rest of the swing is the frame's, and nothing about it touches the
    /// chunk -- which is the whole reason the lid is not a block id.
    #[test]
    fn a_chest_lid_swings_up_while_it_is_open_and_is_handed_back_to_the_mesh_shut() {
        use primitive_shared::types::{BLOCK_CHEST, CHUNK_VOLUME};
        const AT: (i32, i32, i32) = (3, 40, 3);
        let here = ChunkPos::new(0, 0);
        let mut chunks = ChunkManager::new(4);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        blocks[Chunk::index(3, 40, 3)] = BLOCK_CHEST;
        chunks.insert(Chunk { pos: here, blocks });

        assert!(chunks.note_chest_lid(AT, true), "the chunk was not meshed again for a lid leaving it");
        assert_eq!(chunks.lids_in(here), vec![AT], "the mesher was not told to leave the lid out");
        // Told twice -- a second player at the same chest -- changes nothing.
        assert!(!chunks.note_chest_lid(AT, true), "the chunk was meshed again for a lid it had already given up");
        chunks.note_meshed_lids(here, &[AT]);

        // Halfway through the swing and at the top of it.
        assert!(chunks.advance_lids(LID_SWING_SECONDS / 2.0).is_empty());
        let halfway = chunks.open_lids().next().expect("the lid stopped being drawn while it was open").2;
        assert!(halfway > 0.0 && halfway < LID_OPEN, "a lid halfway up is at {halfway}");
        assert!(chunks.advance_lids(LID_SWING_SECONDS).is_empty(), "an open lid was handed back to the mesh");
        assert_eq!(chunks.open_lids().next().map(|lid| lid.2), Some(LID_OPEN), "the lid opened further than it opens");

        chunks.note_chest_lid(AT, false);
        assert!(chunks.advance_lids(LID_SWING_SECONDS / 2.0).is_empty(), "a lid still falling was handed back");
        assert!(chunks.open_lids().next().is_some());
        assert_eq!(chunks.advance_lids(LID_SWING_SECONDS), vec![AT], "a lid that came to rest was not handed back");
        assert!(chunks.lids_in(here).is_empty(), "the next mesh still leaves the shut lid out");
        chunks.note_meshed_lids(here, &[]);
        assert_eq!(chunks.open_lids().count(), 0, "a shut lid is still being drawn by the frame");
    }

    /// **"сундук мерцает при анимации": on every frame of an opening and a
    /// shutting, exactly one of the chunk mesh and the frame draws the lid.**
    ///
    /// The mesh the picture is made of is the one *on screen*, which lands
    /// frames after it is asked for. This plays the handover with meshes
    /// landing up to seven frames late, the way `collect_worker_results`
    /// lands them, and counts the lids drawn on every frame. Before the
    /// handover waited for the mesh it counted two on the way up and none
    /// on the way down.
    #[test]
    fn a_chest_lid_is_drawn_once_on_every_frame_of_its_swing_however_late_the_mesh_lands() {
        use primitive_shared::types::{BLOCK_CHEST, CHUNK_VOLUME};
        const AT: (i32, i32, i32) = (3, 40, 3);
        let here = ChunkPos::new(0, 0);
        for delay in [0usize, 1, 3, 7] {
            let mut chunks = ChunkManager::new(4);
            let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
            blocks[Chunk::index(3, 40, 3)] = BLOCK_CHEST;
            chunks.insert(Chunk { pos: here, blocks });
            // What the mesh on screen leaves out, and the meshes asked for
            // and not landed yet, each with how many frames it has to go.
            let mut on_screen: Vec<(i32, i32, i32)> = Vec::new();
            // Each with the frames it has to go and the lids it leaves out.
            type InFlight = Vec<(usize, Vec<(i32, i32, i32)>)>;
            let mut in_flight: InFlight = Vec::new();
            let frame = 1.0 / 60.0;
            for step in 0..120 {
                if step == 5 && chunks.note_chest_lid(AT, true) {
                    in_flight.push((delay, chunks.lids_in(here)));
                }
                if step == 60 && chunks.note_chest_lid(AT, false) {
                    in_flight.push((delay, chunks.lids_in(here)));
                }
                // Land what is due, then step the swing: the frame loop's own
                // order.
                for (due, left_out) in in_flight.iter() {
                    if *due == 0 {
                        on_screen = left_out.clone();
                        chunks.note_meshed_lids(here, left_out);
                    }
                }
                in_flight.retain(|(due, _)| *due > 0);
                for (due, _) in in_flight.iter_mut() {
                    *due -= 1;
                }
                if !chunks.advance_lids(frame).is_empty() {
                    in_flight.push((delay, chunks.lids_in(here)));
                }
                let by_mesh = usize::from(!on_screen.contains(&AT));
                let by_frame = chunks.open_lids().count();
                assert_eq!(
                    by_mesh + by_frame,
                    1,
                    "a mesh {delay} frames late, frame {step}: the chunk drew {by_mesh} lids and the frame {by_frame}"
                );
            }
            assert_eq!(chunks.open_lids().count(), 0, "the lid was never given back ({delay} frames late)");
            assert!(on_screen.is_empty());
        }
    }

    /// **A loaded chunk of sky keeps no block memory, and one edit in it
    /// costs one section's worth.** The property the manager's storage
    /// was changed for: a flat chunk was 131 KB whatever was in it, so a
    /// streamed horizon of open air cost the same as a mountain.
    #[test]
    fn a_loaded_chunk_of_sky_keeps_no_block_memory_until_something_is_put_in_it() {
        let mut chunks = ChunkManager::new(4);
        chunks.insert(Chunk {
            pos: ChunkPos::new(0, 0),
            blocks: vec![BLOCK_AIR; primitive_shared::types::CHUNK_VOLUME],
        });
        assert_eq!(chunks.heap_bytes(), 0);

        chunks.apply_block_update(3, 100, 3, primitive_shared::types::BLOCK_STONE);
        let one_section = primitive_shared::packed::SECTION_CELLS / 2;
        assert!(
            chunks.heap_bytes() >= one_section && chunks.heap_bytes() < one_section + 64,
            "one block of stone in the sky cost {} bytes",
            chunks.heap_bytes()
        );
        assert_eq!(chunks.block_at(3, 100, 3), Some(primitive_shared::types::BLOCK_STONE));
        assert_eq!(chunks.block_at(3, 101, 3), Some(BLOCK_AIR));
    }

    /// **The regression test for a twelve-second load.**
    ///
    /// Requests used to come out of a `HashSet` in hash order, so the
    /// ground under the player was somewhere in the middle of eighteen
    /// hundred of them -- and the client will not let anybody move
    /// until the three-by-three around them has arrived. Measured at
    /// 12.8 seconds to spawn on a world that streams a hundred and
    /// sixty chunks a second; the fix took it to under a fifth of one.
    ///
    /// What is checked is the property rather than the whole order: the
    /// nine chunks the spawn gate waits on have to be the first nine
    /// asked for.
    #[test]
    fn the_ground_underfoot_is_asked_for_first() {
        let mut chunks = ChunkManager::new(12);
        let centre = ChunkPos::new(40, -17);
        let (to_request, _) = chunks.update(centre, Instant::now());
        assert!(
            to_request.len() > 300,
            "the fixture asked for {} chunks, which is not a stream",
            to_request.len()
        );

        let first_nine: HashSet<ChunkPos> = to_request.iter().take(9).copied().collect();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let under = ChunkPos::new(centre.x + dx, centre.z + dz);
                assert!(
                    first_nine.contains(&under),
                    "{under:?} -- part of the spawn gate -- was not in the first nine                      of {} requests",
                    to_request.len()
                );
            }
        }

        // ...and it is sorted the whole way out, not merely at the
        // front: the horizon should fill in from the middle.
        let distance = |pos: &ChunkPos| {
            let (dx, dz) = ((pos.x - centre.x) as i64, (pos.z - centre.z) as i64);
            dx * dx + dz * dz
        };
        assert!(
            to_request.windows(2).all(|w| distance(&w[0]) <= distance(&w[1])),
            "the request list is not in distance order"
        );
    }
    use primitive_shared::types::{BLOCK_STONE, CHUNK_VOLUME};

    fn chunk(pos: ChunkPos) -> Chunk {
        Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        }
    }

    /// How many chunks the streamed disc holds at a radius.
    ///
    /// Written out here rather than taken from `inside`, so a test of
    /// the shape is a test rather than a restatement: this is the
    /// arithmetic a person would do on paper.
    fn disc(radius: i32) -> usize {
        let mut n = 0;
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let near = dx.abs() <= 1 && dz.abs() <= 1;
                if near || dx * dx + dz * dz <= radius * radius {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn the_streamed_area_is_a_disc_and_not_a_square() {
        // **What the corners cost.** A square reaches `r` chunks along
        // the axes and half again into the corners, and those corners
        // are the ones the fog was already hiding: at the stock distance
        // of six it is fifty-six chunks generated, sent, lit, meshed and
        // drawn to be invisible.
        let mut chunks = ChunkManager::new(6);
        let centre = ChunkPos::new(0, 0);
        let (requested, _) = chunks.update(centre, Instant::now());
        let asked: std::collections::HashSet<ChunkPos> = requested.into_iter().collect();

        assert!(
            asked.contains(&ChunkPos::new(6, 0)) && asked.contains(&ChunkPos::new(0, -6)),
            "the disc must reach its full radius along the axes"
        );
        assert!(
            !asked.contains(&ChunkPos::new(6, 6)) && !asked.contains(&ChunkPos::new(-5, -5)),
            "a corner of the old square was still asked for"
        );
        assert_eq!(asked.len(), disc(6));
        assert!(asked.len() * 4 < 13 * 13 * 3, "the disc saved less than a quarter");
    }

    #[test]
    fn the_nine_chunks_underfoot_are_never_a_matter_of_distance() {
        // At a radius of one a strict disc is a plus, and the four
        // corners it leaves out are four of the nine `is_area_ready`
        // waits for -- so the loading gate would wait for terrain the
        // streamer had decided never to ask for.
        let mut chunks = ChunkManager::new(1);
        let centre = ChunkPos::new(0, 0);
        let (requested, _) = chunks.update(centre, Instant::now());
        for pos in requested {
            chunks.insert(chunk(pos));
        }
        assert_eq!(chunks.loaded_count(), 9, "the 3x3 under the player");
        assert!(chunks.is_area_ready(centre), "the loading gate would never open");
    }

    #[test]
    fn the_fog_is_told_where_the_world_stops() {
        // The number the fade is clamped to, and it is not the one the
        // back of an envelope gives. At a radius of eight the nearest
        // unstreamed cell is (7, 4) -- six chunks and three of clearance
        // away, 107 blocks -- rather than anything on the diagonal, and
        // a fog that ran the full 128 would have open sky under its last
        // sixth.
        let chunks = ChunkManager::new(8);
        let reach = chunks.loaded_radius_blocks();
        assert!(
            (reach - 16.0 * (45f32).sqrt()).abs() < 0.01,
            "the reach at radius 8 should be the (7,4) gap: {reach}"
        );
        assert!(reach < 8.0 * 16.0, "a disc cannot reach as far as its own axis");

        // ...and it follows the setting rather than being worked out
        // once.
        let mut grown = ChunkManager::new(8);
        grown.set_render_distance(16);
        assert!(grown.loaded_radius_blocks() > reach * 1.5);
    }

    #[test]
    fn the_render_distance_can_change_without_losing_the_world() {
        // Rebuilding the manager would drop every chunk and mesh and
        // stream the world back from nothing -- a second of empty sky
        // for a setting the player expects to apply quietly.
        let mut chunks = ChunkManager::new(2);
        let centre = ChunkPos::new(0, 0);
        let now = Instant::now();
        let (requested, _) = chunks.update(centre, now);
        for pos in &requested {
            chunks.insert(Chunk {
                pos: *pos,
                blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
            });
        }
        let before = chunks.loaded_count();
        // The disc at radius 2, plus the four diagonals every radius
        // keeps -- see `inside`. The square would have been 25.
        assert_eq!(before, disc(2), "the disc at distance 2");

        chunks.set_render_distance(3);
        assert_eq!(chunks.render_distance(), 3);
        assert_eq!(chunks.loaded_count(), before, "it threw the world away");

        // And the next scan asks for the new ring even though the
        // player has not moved.
        let (more, unload) = chunks.update(centre, now);
        assert_eq!(more.len(), disc(3) - disc(2), "the new ring was not requested");
        assert!(unload.is_empty());
    }

    #[test]
    fn shrinking_the_render_distance_unloads_the_outer_ring() {
        let mut chunks = ChunkManager::new(3);
        let centre = ChunkPos::new(0, 0);
        let now = Instant::now();
        let (requested, _) = chunks.update(centre, now);
        for pos in &requested {
            chunks.insert(Chunk {
                pos: *pos,
                blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
            });
        }

        chunks.set_render_distance(1);
        let (request, unload) = chunks.update(centre, now);
        assert!(request.is_empty());
        assert_eq!(unload.len(), disc(3) - disc(1), "the outer rings should go");
    }

    #[test]
    fn setting_the_same_render_distance_changes_nothing() {
        let mut chunks = ChunkManager::new(4);
        let now = Instant::now();
        chunks.update(ChunkPos::new(0, 0), now);
        chunks.set_render_distance(4);
        // The scan state is untouched, so a no-op setting does not cost
        // a full rescan every time the player nudges another slider.
        let (request, unload) = chunks.update(ChunkPos::new(0, 0), now);
        assert!(request.is_empty() && unload.is_empty());
    }

    #[test]
    fn a_render_distance_of_zero_is_refused() {
        let mut chunks = ChunkManager::new(4);
        chunks.set_render_distance(0);
        assert!(chunks.render_distance() >= 1, "a world with no chunks is not a world");
    }

    #[test]
    fn a_dropped_request_is_retried() {
        let mut cm = ChunkManager::new(1);
        // Retry window deliberately longer than the 500 ms rescan
        // interval, so the two timers can be told apart below.
        cm.retry_after = Duration::from_millis(1000);
        let now = Instant::now();
        let (first, _) = cm.update(ChunkPos::new(0, 0), now);
        assert_eq!(first.len(), 9, "3x3 ring around the player");

        // Nothing arrived; before the retry window nothing is re-sent.
        let (again, _) = cm.update(ChunkPos::new(0, 0), now + Duration::from_millis(600));
        assert!(again.is_empty(), "retried far too eagerly");

        // After the window, everything still missing is asked for again.
        let (retried, _) = cm.update(ChunkPos::new(0, 0), now + Duration::from_millis(1200));
        assert_eq!(retried.len(), 9);
        assert_eq!(cm.retries_sent, 9);
    }

    #[test]
    fn an_arrived_chunk_is_not_requested_again() {
        let mut cm = ChunkManager::new(1);
        cm.retry_after = Duration::from_millis(10);
        let now = Instant::now();
        cm.update(ChunkPos::new(0, 0), now);
        cm.insert(chunk(ChunkPos::new(0, 0)));
        let (retried, _) = cm.update(ChunkPos::new(0, 0), now + Duration::from_secs(2));
        assert!(!retried.contains(&ChunkPos::new(0, 0)));
        assert_eq!(cm.pending_count(), 8);
    }

    #[test]
    fn walking_away_unloads_and_stops_chasing() {
        let mut cm = ChunkManager::new(1);
        let now = Instant::now();
        cm.update(ChunkPos::new(0, 0), now);
        cm.insert(chunk(ChunkPos::new(0, 0)));

        let (_, unload) = cm.update(ChunkPos::new(50, 50), now + Duration::from_secs(1));
        assert_eq!(unload, vec![ChunkPos::new(0, 0)]);
        // The old pending set must not linger, or we'd keep re-requesting
        // chunks on the other side of the map forever.
        assert!(cm.pending_count() <= 9);
    }

    #[test]
    fn neighbour_lookup_finds_the_right_chunk() {
        let mut cm = ChunkManager::new(2);
        cm.insert(chunk(ChunkPos::new(0, 0)));
        cm.insert(chunk(ChunkPos::new(1, 0)));
        assert!(cm.neighbour(ChunkPos::new(0, 0), 1, 0).is_some());
        assert!(cm.neighbour(ChunkPos::new(0, 0), 0, 1).is_none());
    }

    #[test]
    fn a_no_op_block_update_does_not_request_a_remesh() {
        let mut cm = ChunkManager::new(1);
        let mut c = chunk(ChunkPos::new(0, 0));
        c.set(1, 2, 3, BLOCK_STONE);
        cm.insert(c);
        assert_eq!(cm.apply_block_update(1, 2, 3, BLOCK_STONE), None);
        assert_eq!(
            cm.apply_block_update(1, 2, 3, BLOCK_AIR),
            Some(ChunkPos::new(0, 0))
        );
    }
}

/// Lets the light engine and the mesher read the world in global
/// coordinates. `None` means the chunk isn't loaded, which lighting
/// treats as a wall rather than as air -- light doesn't leak out into
/// the unknown and then have to be taken back when the chunk arrives.
impl BlockSource for ChunkManager {
    #[inline]
    fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
        if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
            return Some(BLOCK_AIR);
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        self.loaded.get(&pos).map(|c| c.get(lx, gy as usize, lz))
    }

    #[inline]
    fn packed_chunk(&self, pos: ChunkPos) -> Option<&PackedChunk> {
        self.loaded.get(&pos).map(|c| &**c)
    }
}

#[cfg(test)]
mod loading_tests {
    use super::*;
    use primitive_shared::types::{Chunk, CHUNK_VOLUME};

    fn chunk_at(pos: ChunkPos) -> Chunk {
        Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        }
    }

    #[test]
    fn the_area_is_not_ready_until_all_nine_chunks_are_there() {
        let mut cm = ChunkManager::new(2);
        let centre = ChunkPos::new(0, 0);
        assert!(!cm.is_area_ready(centre), "nothing loaded yet");
        assert_eq!(cm.spawn_area_progress(centre), (0, 9));

        for dx in -1..=1 {
            for dz in -1..=1 {
                cm.insert(chunk_at(ChunkPos::new(dx, dz)));
            }
        }
        assert_eq!(cm.spawn_area_progress(centre), (9, 9));
        assert!(cm.is_area_ready(centre));
    }

    #[test]
    fn a_hole_in_the_neighbourhood_keeps_the_player_waiting() {
        // The centre chunk alone isn't enough: stepping one block sideways
        // would drop the player into an unloaded chunk, which reads as air.
        let mut cm = ChunkManager::new(2);
        cm.insert(chunk_at(ChunkPos::new(0, 0)));
        assert!(!cm.is_area_ready(ChunkPos::new(0, 0)));
    }
}

#[cfg(test)]
mod arrival_tests {
    use super::*;
    use primitive_shared::types::{Chunk, CHUNK_VOLUME};

    #[test]
    fn an_arrived_but_unintegrated_chunk_is_not_requested_again() {
        // Regression: deferring integration made the retry logic think
        // these chunks were still missing, so it asked for them again
        // and again while they waited in the queue.
        let mut cm = ChunkManager::new(1);
        cm.retry_after = Duration::from_millis(1);
        let now = Instant::now();
        let (first, _) = cm.update(ChunkPos::new(0, 0), now);
        assert_eq!(first.len(), 9);

        for pos in &first {
            cm.note_arrival(*pos);
        }

        let (again, _) = cm.update(ChunkPos::new(0, 0), now + Duration::from_secs(5));
        assert!(
            again.is_empty(),
            "re-requested {} chunks that had already arrived",
            again.len()
        );
    }

    #[test]
    fn integration_clears_the_arrived_marker() {
        let mut cm = ChunkManager::new(1);
        let pos = ChunkPos::new(0, 0);
        cm.note_arrival(pos);
        assert!(cm.arrived.contains(&pos));
        cm.insert(Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        });
        assert!(cm.arrived.is_empty());
        assert!(cm.is_loaded(pos));
    }
}

#[cfg(test)]
mod settling_tests {
    use super::*;
    use primitive_shared::types::{Chunk, CHUNK_VOLUME};

    fn air_chunk(pos: ChunkPos) -> Chunk {
        Chunk {
            pos,
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        }
    }

    #[test]
    fn a_chunk_waits_for_its_neighbours_before_meshing() {
        let mut cm = ChunkManager::new(4);
        let centre = ChunkPos::new(0, 0);
        cm.insert(air_chunk(centre));
        assert!(
            !cm.neighbourhood_settled(centre, centre),
            "should wait while neighbours are still streaming in"
        );

        for (dx, dz) in NEIGHBOUR_OFFSETS {
            cm.insert(air_chunk(ChunkPos::new(dx, dz)));
        }
        assert!(cm.neighbourhood_settled(centre, centre));
    }

    #[test]
    fn the_outermost_ring_does_not_wait_forever() {
        // Its outward neighbours are outside the render distance and
        // will never arrive; without this the edge of the world would
        // never be drawn.
        let mut cm = ChunkManager::new(2);
        let centre = ChunkPos::new(0, 0);
        for dx in -2..=2 {
            for dz in -2..=2 {
                cm.insert(air_chunk(ChunkPos::new(dx, dz)));
            }
        }
        let edge = ChunkPos::new(2, 2);
        assert!(
            cm.neighbourhood_settled(edge, centre),
            "the outer ring must mesh once everything inside it has arrived"
        );
    }

    #[test]
    fn a_gap_inside_the_radius_still_blocks() {
        let mut cm = ChunkManager::new(3);
        let centre = ChunkPos::new(0, 0);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if (dx, dz) == (1, 1) {
                    continue; // the one still in flight
                }
                cm.insert(air_chunk(ChunkPos::new(dx, dz)));
            }
        }
        assert!(!cm.neighbourhood_settled(centre, centre));
    }
}
