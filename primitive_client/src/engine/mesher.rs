//! Off-thread chunk meshing.
//!
//! ## Why
//!
//! Meshing a chunk costs ~14 ms in a debug build. It used to run on the
//! main thread under a 4 ms budget, which the budget could not actually
//! enforce -- the check happens *between* chunks, so a single chunk
//! blew straight through it. The result was exactly the reported
//! symptom: frame times that swing between fine and terrible depending
//! on whether the frame happened to mesh anything.
//!
//! It also made breaking a block feel broken. The edit marked the chunk
//! dirty, the chunk went to the back of a queue that could be a hundred
//! deep during terrain streaming, and the rebuild landed seconds later.
//!
//! ## Split
//!
//! The main thread does the cheap part: filling a `Neighbourhood` (324
//! column lookups -- the chunk plus one block of padding, blocks and
//! light). Worker threads do the expensive part: iterating half a
//! million cells to emit faces, light and ambient occlusion.
//!
//! ## Buffers go round, not away
//!
//! Both the neighbourhood (~60 KB) and the output vertex/index buffers
//! travel to the worker and come back, and the main thread keeps them in
//! a pool. Nothing is allocated per chunk in the steady state; without
//! this, meshing at a few chunks per second would churn megabytes.
//!
//! ## Priority
//!
//! Player edits are submitted ahead of streaming work. A chunk the
//! player just changed is the one they're looking at.

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

use primitive_shared::types::ChunkPos;

use primitive_shared::lighting::compute_isolated;
use primitive_shared::types::Chunk;
use primitive_shared::worldgen::WorldGen;

use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::texture::FaceLayers;

enum Job {
    Mesh {
        pos: ChunkPos,
        /// The chunk's edit counter when this snapshot was taken.
        version: u64,
        cache: Box<Neighbourhood>,
        out: MeshBuffers,
        /// The world's own generator, for the climate that colours
        /// foliage. Carried per job rather than held by the worker
        /// because the seed only arrives with the handshake, and
        /// reconnecting to a different world has to change it.
        world: Arc<WorldGen>,
    },
    /// Light a chunk in isolation. Pure computation over the chunk's
    /// blocks -- the expensive half of chunk integration, and the half
    /// that doesn't touch the shared light map.
    ///
    /// The chunk is shared rather than copied: it is 32 KB, this runs
    /// once per chunk as terrain streams, and nothing here writes to it.
    Light {
        pos: ChunkPos,
        chunk: Arc<Chunk>,
    },
}

pub enum Finished {
    Mesh {
        pos: ChunkPos,
        version: u64,
        buffers: MeshBuffers,
        cache: Box<Neighbourhood>,
    },
    Light {
        pos: ChunkPos,
        data: Vec<u8>,
    },
}

pub struct Mesher {
    jobs: Sender<Job>,
    finished: Receiver<Finished>,
    /// Boxed, and clippy is wrong about it here: the *point* is that a
    /// `Neighbourhood` keeps its address while it travels to a worker
    /// and back, so the pool hands out the same allocation rather than
    /// moving 60 KB into and out of a vector every time a chunk is
    /// meshed.
    #[allow(clippy::vec_box)]
    cache_pool: Vec<Box<Neighbourhood>>,
    buffer_pool: Vec<MeshBuffers>,
    in_flight: usize,
    lighting_in_flight: usize,
    workers: usize,
    /// The generator for the world currently being played, shared with
    /// every job in flight. See `set_world`.
    world: Arc<WorldGen>,
}

impl Mesher {
    /// `configured` is the thread count from settings; 0 means "decide
    /// automatically".
    pub fn new(layers: FaceLayers, configured: usize) -> Self {
        // Use the whole machine. One core is left for the main thread,
        // which still renders, runs physics and drives the network -- on
        // an 8-core box that's 7 workers rather than the 4 this used to
        // cap at. The cap existed out of caution about idle threads
        // holding pooled buffers; the pools are bounded per worker
        // instead, which is the direct fix for that concern.
        let workers = if configured > 0 {
            configured.clamp(1, 64)
        } else {
            std::thread::available_parallelism()
                .map(|n| n.get().saturating_sub(1).max(1))
                .unwrap_or(1)
        };

        let (job_tx, job_rx) = channel::<Job>();
        let (done_tx, done_rx) = channel::<Finished>();
        let job_rx = Arc::new(Mutex::new(job_rx));

        for index in 0..workers {
            let job_rx = Arc::clone(&job_rx);
            let done_tx = done_tx.clone();
            let layers = layers.clone();
            std::thread::Builder::new()
                .name(format!("primitive-mesher-{index}"))
                .spawn(move || loop {
                    // Hold the lock only long enough to take a job, not
                    // for the meshing itself -- otherwise the workers
                    // would serialise on each other.
                    let job = {
                        let guard = job_rx.lock().unwrap_or_else(|e| e.into_inner());
                        guard.recv()
                    };
                    let Ok(job) = job else {
                        break; // main thread went away
                    };

                    let result = match job {
                        Job::Mesh {
                            pos,
                            version,
                            cache,
                            mut out,
                            world,
                        } => {
                            build_mesh(pos, &cache, &layers, &world, &mut out);
                            Finished::Mesh {
                                pos,
                                version,
                                buffers: out,
                                cache,
                            }
                        }
                        Job::Light { pos, chunk } => Finished::Light {
                            pos,
                            data: compute_isolated(&chunk.blocks),
                        },
                    };

                    if done_tx.send(result).is_err() {
                        break;
                    }
                })
                .expect("failed to spawn mesher thread");
        }

        println!("chunk meshing and lighting on {workers} worker thread(s)");

        Self {
            jobs: job_tx,
            finished: done_rx,
            cache_pool: Vec::new(),
            buffer_pool: Vec::new(),
            in_flight: 0,
            lighting_in_flight: 0,
            workers,
            // Replaced by `set_world` as soon as a handshake names the
            // real seed. Until then nothing has been submitted, so the
            // placeholder never colours anything.
            world: Arc::new(WorldGen::new(0)),
        }
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    /// Points the workers at the world now being played.
    ///
    /// Jobs already in flight keep the generator they were sent with,
    /// which is the right answer: they are meshing chunks of the world
    /// that generator describes, and the results are thrown away on
    /// disconnect anyway.
    pub fn set_world(&mut self, world: WorldGen) {
        self.world = Arc::new(world);
    }

    /// Mesh jobs the workers haven't returned yet.
    pub fn in_flight(&self) -> usize {
        self.in_flight
    }

    /// Lighting jobs outstanding.
    pub fn lighting_in_flight(&self) -> usize {
        self.lighting_in_flight
    }

    /// A scratch neighbourhood to fill, taken from the pool.
    pub fn take_cache(&mut self) -> Box<Neighbourhood> {
        self.cache_pool
            .pop()
            .unwrap_or_else(|| Box::new(Neighbourhood::default()))
    }

    /// Hands a filled neighbourhood to a worker.
    /// `version` is the chunk's edit counter at the moment the snapshot
    /// was taken; it comes back with the result so the caller can throw
    /// away a mesh that the world has since moved past.
    pub fn submit(&mut self, pos: ChunkPos, version: u64, cache: Box<Neighbourhood>) {
        let out = self.buffer_pool.pop().unwrap_or_default();
        if self
            .jobs
            .send(Job::Mesh {
                pos,
                version,
                cache,
                out,
                world: Arc::clone(&self.world),
            })
            .is_ok()
        {
            self.in_flight += 1;
        }
    }

    /// Queues the pure half of lighting a newly arrived chunk.
    pub fn submit_lighting(&mut self, pos: ChunkPos, chunk: Arc<Chunk>) {
        if self.jobs.send(Job::Light { pos, chunk }).is_ok() {
            self.lighting_in_flight += 1;
        }
    }

    /// Collects whatever the workers have finished. Never blocks.
    // Not a `while let`: the two error arms mean different things (empty
    // now, and gone for good) and both end the loop, which is exactly
    // what a `match` says and a `while let` hides.
    #[allow(clippy::while_let_loop)]
    pub fn collect(&mut self) -> Vec<Finished> {
        let mut done = Vec::new();
        loop {
            match self.finished.try_recv() {
                Ok(finished) => {
                    match &finished {
                        Finished::Mesh { .. } => {
                            self.in_flight = self.in_flight.saturating_sub(1)
                        }
                        Finished::Light { .. } => {
                            self.lighting_in_flight = self.lighting_in_flight.saturating_sub(1)
                        }
                    }
                    done.push(finished);
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        done
    }

    /// Returns a finished job's memory to the pool, after the caller has
    /// uploaded the mesh to the GPU.
    pub fn recycle(&mut self, cache: Box<Neighbourhood>, mut buffers: MeshBuffers) {
        // Cap the pools: a burst shouldn't leave a permanently inflated
        // memory footprint.
        if self.cache_pool.len() < self.workers * 2 {
            self.cache_pool.push(cache);
        }
        if self.buffer_pool.len() < self.workers * 2 {
            buffers.clear();
            self.buffer_pool.push(buffers);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pool with no textures configured: every layer resolves to 0,
    /// which is fine -- these tests are about the pipeline, not pixels.
    fn test_layers() -> FaceLayers {
        crate::engine::texture::FaceLayers::empty_for_test()
    }

    /// Polls until a result arrives, since the workers are real threads.
    fn wait_for_one(mesher: &mut Mesher) -> Finished {
        for _ in 0..400 {
            if let Some(first) = mesher.collect().into_iter().next() {
                return first;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("worker never returned a result");
    }

    #[test]
    fn a_submitted_chunk_comes_back_meshed() {
        let mut mesher = Mesher::new(test_layers(), 2);
        let cache = mesher.take_cache();
        mesher.submit(ChunkPos::new(0, 0), 1, cache);
        assert_eq!(mesher.in_flight(), 1);

        match wait_for_one(&mut mesher) {
            Finished::Mesh { pos, version, .. } => {
                assert_eq!(pos, ChunkPos::new(0, 0));
                assert_eq!(version, 1, "the version must come back with the result");
            }
            Finished::Light { .. } => panic!("expected a mesh result"),
        }
        assert_eq!(mesher.in_flight(), 0);
    }

    #[test]
    fn lighting_runs_on_the_workers_too() {
        use primitive_shared::types::{BLOCK_STONE, CHUNK_VOLUME};

        let mut mesher = Mesher::new(test_layers(), 2);
        let mut blocks = vec![primitive_shared::types::BLOCK_AIR; CHUNK_VOLUME];
        // A floor, so the result isn't trivially uniform.
        for cell in blocks.iter_mut().take(16 * 16) {
            *cell = BLOCK_STONE;
        }
        let chunk = Arc::new(Chunk {
            pos: ChunkPos::new(2, 3),
            blocks,
        });
        mesher.submit_lighting(ChunkPos::new(2, 3), chunk);
        assert_eq!(mesher.lighting_in_flight(), 1);

        match wait_for_one(&mut mesher) {
            Finished::Light { pos, data } => {
                assert_eq!(pos, ChunkPos::new(2, 3));
                assert_eq!(data.len(), CHUNK_VOLUME);
                assert!(
                    data.iter().any(|cell| cell & 0x0F > 0),
                    "open sky above the floor should be lit"
                );
            }
            Finished::Mesh { .. } => panic!("expected a lighting result"),
        }
        assert_eq!(mesher.lighting_in_flight(), 0);
    }

    #[test]
    fn buffers_are_recycled_rather_than_reallocated() {
        let mut mesher = Mesher::new(test_layers(), 2);
        let cache = mesher.take_cache();
        mesher.submit(ChunkPos::new(1, 1), 1, cache);

        match wait_for_one(&mut mesher) {
            Finished::Mesh { cache, buffers, .. } => mesher.recycle(cache, buffers),
            Finished::Light { .. } => panic!("expected a mesh result"),
        }

        assert_eq!(mesher.cache_pool.len(), 1, "cache should return to the pool");
        assert_eq!(mesher.buffer_pool.len(), 1, "buffers should return too");

        let reused = mesher.take_cache();
        assert!(mesher.cache_pool.is_empty());
        drop(reused);
    }

    #[test]
    fn collect_does_not_block_when_nothing_is_ready() {
        let mut mesher = Mesher::new(test_layers(), 2);
        assert!(mesher.collect().is_empty());
    }

    #[test]
    fn the_pool_uses_the_machine_when_left_to_decide() {
        // 0 means "auto": every core but one, and never zero threads.
        let auto = Mesher::new(test_layers(), 0);
        let expected = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1).max(1))
            .unwrap_or(1);
        assert_eq!(auto.workers(), expected);
        assert!(auto.workers() >= 1);
    }

    #[test]
    fn an_explicit_thread_count_is_honoured_and_clamped() {
        assert_eq!(Mesher::new(test_layers(), 3).workers(), 3);
        // Nonsense values must not spawn thousands of threads.
        assert!(Mesher::new(test_layers(), 10_000).workers() <= 64);
    }
}
