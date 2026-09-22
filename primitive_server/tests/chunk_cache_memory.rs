//! What the server's chunk cache costs in memory, counted by the
//! allocator.
//!
//! ```text
//! cargo test --release -p primitive_server --no-default-features \
//!     --test chunk_cache_memory -- --ignored --nocapture
//! ```
//!
//! **The same process as the client in singleplayer**, which is why this
//! number matters twice: a local world is a real server on loopback, and
//! its cache is a second copy of everything the player can see -- plus,
//! once they have walked about, everything they *have* seen, up to
//! `max_cached_chunks`.
//!
//! Counted rather than estimated, the way `primitive_shared`'s
//! `chunk_memory` test is: every allocation goes through `Counting`, and
//! the cache's share is what the counter says before and after the chunks
//! go in. Generating them happens first, so the generator's own memo is
//! not billed to the cache.
//!
//! **Measured before the cache was packed**, holding `Arc<Chunk>`:
//!
//! ```text
//! cache: 131160 B/chunk, 224.3 MB
//! a full default cache of 8192 chunks would be 1025 MB
//! ```

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use primitive_server::logic::world::World;
use primitive_shared::types::{Chunk, ChunkPos};
use primitive_shared::worldgen::WorldGen;

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const RADIUS: i32 = 24;
const MB: f64 = 1024.0 * 1024.0;

#[test]
#[ignore = "a measurement, not an assertion -- run it explicitly, in release"]
fn what_the_server_cache_costs_in_memory() {
    let seed = 4242;
    let gen = WorldGen::new(seed);
    let (sx, sz) = gen.spawn_column();
    let centre = ChunkPos::from_global(sx, sz).0;
    let mut positions = Vec::new();
    for dx in -RADIUS..=RADIUS {
        for dz in -RADIUS..=RADIUS {
            if (dx.abs() <= 1 && dz.abs() <= 1) || dx * dx + dz * dz <= RADIUS * RADIUS {
                positions.push(ChunkPos::new(centre.x + dx, centre.z + dz));
            }
        }
    }
    let count = positions.len();

    // The default cache, so nothing is evicted and every chunk counts.
    let world = World::new(seed, 8192);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per = count.div_ceil(threads);
    let generate_all = || -> Vec<Chunk> {
        std::thread::scope(|scope| {
            let world = &world;
            let handles: Vec<_> = positions
                .chunks(per)
                .map(|batch| {
                    scope.spawn(move || batch.iter().map(|&p| world.generate(p)).collect::<Vec<_>>())
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
        })
    };
    // Twice, and the first lot thrown away: the generator memoises
    // columns and lakes as it goes, and that memo is bounded and belongs
    // to the generator. Warmed first, it is the same size before and
    // after the reading below, and the difference is the cache alone.
    drop(generate_all());
    // Read *before* the chunks exist, not before they go in: generating
    // is what allocates their blocks, and inserting only moves them.
    // Reading afterwards weighed the hash map and nothing else.
    let before = LIVE.load(Ordering::Relaxed);
    let generated = generate_all();
    let started = std::time::Instant::now();
    for chunk in generated {
        world.insert(chunk);
    }
    let insert = started.elapsed();
    let bytes = LIVE.load(Ordering::Relaxed) - before;
    let per_chunk = bytes as f64 / count as f64;

    println!("{count} chunks, render distance {RADIUS}, seed {seed}");
    println!("cache: {per_chunk:.0} B/chunk, {:.1} MB", bytes as f64 / MB);
    println!(
        "a full default cache of 8192 chunks would be {:.0} MB",
        per_chunk * 8192.0 / MB
    );
    println!(
        "insert: {:.3} ms/chunk",
        insert.as_secs_f64() * 1000.0 / count as f64
    );
    assert_eq!(world.stats().cached_chunks, count);
}
