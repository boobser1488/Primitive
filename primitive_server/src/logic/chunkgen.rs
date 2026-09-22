//! The chunk generation queue: one place where terrain gets made.
//!
//! ## What was here before, and why it stopped being enough
//!
//! Generation used to happen inside each player's chunk pump:
//!
//! ```text
//! for _ in 0..budget {
//!     let pos = queue.try_recv()?;
//!     let chunk = spawn_blocking(move || world.generate(pos)).await;
//!     send(chunk);
//! }
//! ```
//!
//! Three things are wrong with that, and only the first is obvious.
//!
//! 1. **It is serial.** `.await` inside the loop means chunk *n+1* does
//!    not start until chunk *n* has come back from the blocking pool. A
//!    machine with eight cores generated a single player's world on
//!    one of them, at one chunk per round trip through the runtime.
//!    Joining a world with a view distance of eight asks for 289 chunks;
//!    at roughly a millisecond of noise apiece plus a thread hop, that
//!    is most of a second of pure CPU spent one core at a time, on top
//!    of a send budget that spreads it over several more.
//! 2. **It duplicates work.** Two players standing near each other ask
//!    for the same chunks. `World::insert` resolves the race correctly
//!    -- whoever arrives second throws their copy away -- but both of
//!    them *generated* it. On a busy server that is the commonest chunk
//!    access there is.
//! 3. **It has no memory.** A chunk that was asked for, generated, and
//!    then evicted before it could be sent gets generated again, and
//!    there is nothing anywhere that knows a request is already in
//!    flight.
//!
//! ## The shape now
//!
//! One service, owned by the [`Context`](crate::Context), holding a
//! priority queue of positions and a pool of plain OS threads.
//!
//! - **Anyone may ask.** [`ChunkService::request`] is non-blocking and
//!   idempotent: asking for a chunk that is cached, queued, or already
//!   being generated does nothing at all.
//! - **Nothing waits.** The chunk pump asks for what it wants and then
//!   sends whatever happens to be ready. A chunk that is not ready this
//!   tick is ready in one of the next few, and the pump comes round 20
//!   times a second.
//! - **Nearest first.** Requests carry the distance from the player who
//!   wanted them, and the queue is drained by priority. The ground under
//!   somebody's feet beats the horizon behind them, which is what makes
//!   a join feel fast rather than merely *be* fast.
//! - **Work is done once.** `queued` and `in_flight` between them mean a
//!   position is in the system exactly once however many players want
//!   it, and everyone reads the one copy out of the world cache
//!   afterwards.
//!
//! ## Why OS threads and not `spawn_blocking`
//!
//! Tokio's blocking pool is shared with everything else that blocks --
//! the world save, the profile save, the chest save -- and it is sized
//! for tasks that are *waiting*, not for tasks that are burning a core.
//! Terrain generation is the second kind: it is pure arithmetic, it
//! never yields, and the right number of them to run at once is "one per
//! spare core", not five hundred. A dedicated pool of that size is both
//! simpler to reason about and impossible to starve a save with.
//!
//! It also means the generator threads are not on the async runtime at
//! all, so no amount of terrain can delay a snapshot, a keepalive or a
//! block update -- which was the other half of the original problem.

use std::collections::{BinaryHeap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use primitive_shared::types::ChunkPos;

use crate::logic::world::World;

/// A queued position and how badly it is wanted.
///
/// `Ord` is deliberately reversed on the distance so a `BinaryHeap` --
/// which is a max-heap -- pops the *nearest* chunk. The tiebreak on the
/// position keeps the order total, which `BinaryHeap` requires and which
/// also makes the drain order deterministic for a given set of requests.
#[derive(PartialEq, Eq)]
struct Wanted {
    /// Squared chunk distance from whoever asked. Squared because the
    /// square root would change nothing about the ordering.
    distance: i64,
    pos: ChunkPos,
}

impl Ord for Wanted {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .distance
            .cmp(&self.distance)
            .then_with(|| other.pos.x.cmp(&self.pos.x))
            .then_with(|| other.pos.z.cmp(&self.pos.z))
    }
}

impl PartialOrd for Wanted {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Default)]
struct Queue {
    heap: BinaryHeap<Wanted>,
    /// What is in the heap. A heap cannot be searched, and without this
    /// a player walking back and forth over a border would queue the
    /// same chunk once a tick forever.
    queued: HashSet<ChunkPos>,
    /// What a worker has taken and not yet published. Kept separate from
    /// `queued` so a request that arrives mid-generation is dropped
    /// rather than re-queued behind the worker already doing it.
    in_flight: HashSet<ChunkPos>,
    stopping: bool,
}

/// How many positions the queue will hold before it starts refusing the
/// far ones.
///
/// Reached only by something pathological -- a hundred players all
/// teleporting at once, or a plugin asking for a continent. What it
/// buys is that the failure is "the horizon takes another second"
/// rather than "the server's memory is a list of chunk coordinates".
///
/// The refusal drops the *furthest* entry rather than the new one,
/// because the new one is usually nearer: a queue that rejected new work
/// when full would serve a stale horizon in preference to the ground a
/// player is standing on.
const MAX_QUEUED: usize = 16_384;

pub struct ChunkService {
    world: Arc<World>,
    queue: Mutex<Queue>,
    /// Woken when work arrives or the service is stopping. A `Condvar`
    /// rather than a channel because the queue is a priority queue: a
    /// channel would hand the workers positions in arrival order, which
    /// is exactly the order that makes a join feel slow.
    work: Condvar,
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
    stopping: AtomicBool,
    generated: AtomicU64,
    deduplicated: AtomicU64,
    dropped: AtomicU64,
}

/// What the service is doing, for `/stats` and the client's debug panel.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChunkServiceStats {
    pub queued: usize,
    pub in_flight: usize,
    pub workers: usize,
    pub generated: u64,
    /// Requests that were already cached, queued or in flight -- work
    /// that would have been done twice under the old shape.
    pub deduplicated: u64,
    pub dropped: u64,
}

impl ChunkService {
    /// Starts the pool. `threads` of zero means "choose", which is one
    /// per core less two: one for the async runtime that is driving
    /// every socket, and one for the machine to stay usable on. Never
    /// fewer than one, because a server with no generator threads is a
    /// server with no world.
    pub fn start(world: Arc<World>, threads: usize) -> Arc<Self> {
        let threads = if threads > 0 {
            threads
        } else {
            std::thread::available_parallelism()
                .map(|n| n.get().saturating_sub(2))
                .unwrap_or(1)
                .max(1)
        };
        let service = Arc::new(Self {
            world,
            queue: Mutex::new(Queue::default()),
            work: Condvar::new(),
            workers: Mutex::new(Vec::new()),
            stopping: AtomicBool::new(false),
            generated: AtomicU64::new(0),
            deduplicated: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        });
        let mut handles = Vec::with_capacity(threads);
        for n in 0..threads {
            let service = Arc::clone(&service);
            let spawned = std::thread::Builder::new()
                .name(format!("chunkgen-{n}"))
                .spawn(move || service.worker());
            match spawned {
                Ok(handle) => handles.push(handle),
                // A machine that will not give us a thread still has to
                // run: `request` falls back to generating inline, which
                // is exactly what the old code did on every chunk.
                Err(e) => eprintln!("[world] could not start a generator thread: {e}"),
            }
        }
        *service.workers.lock().unwrap_or_else(|e| e.into_inner()) = handles;
        service
    }

    /// Asks for a chunk, nearest-first, and returns immediately.
    ///
    /// Does nothing at all if the chunk is already cached, already
    /// queued, or already being generated -- which is the common case
    /// on a server where several people are standing in the same
    /// valley. `distance` is in chunks, from whoever wants it.
    pub fn request(&self, pos: ChunkPos, distance: i64) {
        if self.world.cached(pos).is_some() {
            self.deduplicated.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.stopping {
            return;
        }
        if queue.queued.contains(&pos) || queue.in_flight.contains(&pos) {
            self.deduplicated.fetch_add(1, Ordering::Relaxed);
            return;
        }
        queue.queued.insert(pos);
        queue.heap.push(Wanted { distance, pos });
        // Over the cap: throw away the furthest thing in the queue,
        // which may be the entry that was just pushed. `BinaryHeap` has
        // no "pop min", so this rebuilds -- which is fine precisely
        // because it only ever happens once the queue is enormous, and
        // then only once per further request.
        if queue.heap.len() > MAX_QUEUED {
            let mut all: Vec<Wanted> = queue.heap.drain().collect();
            // Nearest first, so the tail is the furthest.
            all.sort_by_key(|w| w.distance);
            for far in all.drain(MAX_QUEUED..) {
                queue.queued.remove(&far.pos);
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            queue.heap = all.into_iter().collect();
        }
        drop(queue);
        self.work.notify_one();
    }

    /// Asks for several at once, which is what a chunk pump has.
    ///
    /// One lock and one wake for the batch rather than one apiece: a
    /// player crossing a chunk border asks for a whole new rank of them
    /// in the same tick.
    pub fn request_many(&self, wanted: impl IntoIterator<Item = (ChunkPos, i64)>) {
        let mut woke = 0usize;
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.stopping {
            return;
        }
        for (pos, distance) in wanted {
            if queue.queued.contains(&pos) || queue.in_flight.contains(&pos) {
                self.deduplicated.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            // The cache check is inside the loop and after the queue
            // check, in that order, because the queue check is two hash
            // lookups on a lock we are already holding and the cache
            // check is a shard lock of its own.
            if self.world.cached(pos).is_some() {
                self.deduplicated.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            queue.queued.insert(pos);
            queue.heap.push(Wanted { distance, pos });
            woke += 1;
        }
        if queue.heap.len() > MAX_QUEUED {
            let mut all: Vec<Wanted> = queue.heap.drain().collect();
            all.sort_by_key(|w| w.distance);
            for far in all.drain(MAX_QUEUED..) {
                queue.queued.remove(&far.pos);
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            queue.heap = all.into_iter().collect();
        }
        drop(queue);
        for _ in 0..woke.min(64) {
            self.work.notify_one();
        }
    }

    /// The chunk if it is ready, generating it here and now if the pool
    /// could not be started at all.
    ///
    /// The fallback matters: a machine that refused every thread would
    /// otherwise never produce a chunk, and a world that never loads is
    /// worse than a world that loads slowly.
    pub fn take(&self, pos: ChunkPos) -> Option<Arc<primitive_shared::packed::PackedChunk>> {
        if let Some(chunk) = self.world.cached(pos) {
            return Some(chunk);
        }
        if self.worker_count() == 0 {
            let chunk = self.world.generate(pos);
            self.generated.fetch_add(1, Ordering::Relaxed);
            return Some(self.world.insert(chunk));
        }
        None
    }

    pub fn worker_count(&self) -> usize {
        self.workers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub fn stats(&self) -> ChunkServiceStats {
        let queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        ChunkServiceStats {
            queued: queue.heap.len(),
            in_flight: queue.in_flight.len(),
            workers: self
                .workers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
            generated: self.generated.load(Ordering::Relaxed),
            deduplicated: self.deduplicated.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
        }
    }

    /// Asks the pool to wind up and waits for it.
    ///
    /// Called from `shutdown`, and it has to be *waited for*: a
    /// singleplayer session that ended with generator threads still
    /// holding an `Arc<World>` would keep the whole world alive after
    /// the player went back to the menu, and the next world they opened
    /// would be sharing a machine with the last one's terrain.
    pub fn stop(&self) {
        if self.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        {
            let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.stopping = true;
            queue.heap.clear();
            queue.queued.clear();
        }
        self.work.notify_all();
        let handles: Vec<_> = std::mem::take(&mut *self.workers.lock().unwrap_or_else(|e| e.into_inner()));
        for handle in handles {
            let _ = handle.join();
        }
    }

    /// One generator thread: take the nearest wanted chunk, make it,
    /// publish it, repeat.
    fn worker(self: Arc<Self>) {
        loop {
            let pos = {
                let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if queue.stopping {
                        return;
                    }
                    if let Some(wanted) = queue.heap.pop() {
                        queue.queued.remove(&wanted.pos);
                        queue.in_flight.insert(wanted.pos);
                        break wanted.pos;
                    }
                    queue = self.work.wait(queue).unwrap_or_else(|e| e.into_inner());
                }
            };

            // **Outside every lock**, which is the whole point of the
            // module: this is a millisecond of noise, and nothing else
            // in the server may be waiting on it.
            let chunk = self.world.generate(pos);
            self.world.insert(chunk);
            self.generated.fetch_add(1, Ordering::Relaxed);

            let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.in_flight.remove(&pos);
        }
    }
}

impl Drop for ChunkService {
    fn drop(&mut self) {
        // Belt and braces. `stop` is called explicitly on shutdown; this
        // catches the paths that drop a service without one -- a failed
        // start, and every test that builds a context and lets it go.
        if !self.stopping.load(Ordering::SeqCst) {
            let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.stopping = true;
            drop(queue);
            self.work.notify_all();
            let handles: Vec<_> =
                std::mem::take(&mut *self.workers.lock().unwrap_or_else(|e| e.into_inner()));
            for handle in handles {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::worldgen::Preset;

    fn world() -> Arc<World> {
        Arc::new(World::with_preset(4242, Preset::Normal, 4096))
    }

    /// Wait until `f` is true or the deadline passes. Generation is
    /// asynchronous by construction, so every assertion about it is an
    /// assertion about what happens *eventually*.
    fn eventually(mut f: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            if f() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn a_requested_chunk_turns_up_in_the_world() {
        let world = world();
        let service = ChunkService::start(Arc::clone(&world), 2);
        let pos = ChunkPos::new(3, -4);
        assert!(world.cached(pos).is_none());
        service.request(pos, 0);
        assert!(
            eventually(|| world.cached(pos).is_some()),
            "the chunk was never generated"
        );
        service.stop();
    }

    #[test]
    fn asking_twice_generates_once() {
        // The dedup that the old per-player pump did not have: two
        // players standing in the same valley asked for the same
        // terrain and both machines' worth of noise got run.
        let world = world();
        let service = ChunkService::start(Arc::clone(&world), 1);
        let positions: Vec<ChunkPos> = (0..24).map(|n| ChunkPos::new(n, 0)).collect();
        for _ in 0..4 {
            service.request_many(positions.iter().map(|&p| (p, 0)));
        }
        assert!(eventually(|| positions
            .iter()
            .all(|&p| world.cached(p).is_some())));
        service.stop();
        let stats = service.stats();
        assert_eq!(
            stats.generated, 24,
            "24 chunks asked for four times were generated {} times",
            stats.generated
        );
        assert!(stats.deduplicated >= 72, "{stats:?}");
    }

    #[test]
    fn the_nearest_chunk_is_generated_first() {
        // One worker, so the order is observable at all. The far chunks
        // are queued first and the near one last, which is the case
        // that a plain FIFO gets wrong.
        let world = world();
        let service = ChunkService::start(Arc::clone(&world), 1);
        let mut wanted: Vec<(ChunkPos, i64)> = (2..40)
            .map(|n| (ChunkPos::new(n, 7), (n * n) as i64))
            .collect();
        wanted.push((ChunkPos::new(0, 7), 0));
        service.request_many(wanted);
        let near = ChunkPos::new(0, 7);
        let far = ChunkPos::new(39, 7);
        assert!(eventually(|| world.cached(near).is_some()));
        assert!(
            world.cached(far).is_none(),
            "the horizon was generated before the ground underfoot"
        );
        service.stop();
    }

    #[test]
    fn a_service_with_no_threads_still_answers() {
        // The fallback path. A machine that refuses threads must still
        // produce a world, slowly, rather than none at all.
        let world = world();
        let service = ChunkService::start(Arc::clone(&world), 1);
        service.stop(); // now it has no workers
        let pos = ChunkPos::new(-9, 9);
        assert!(service.take(pos).is_some());
        assert!(world.cached(pos).is_some());
    }

    #[test]
    fn stopping_twice_is_harmless() {
        let service = ChunkService::start(world(), 2);
        service.stop();
        service.stop();
    }
}
