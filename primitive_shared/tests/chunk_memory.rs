//! What a client's world costs in memory, counted by the allocator.
//!
//! ```text
//! cargo test --release -p primitive_shared --test chunk_memory -- --ignored --nocapture
//! ```
//!
//! **Counted, not estimated.** Every byte this test binary allocates goes
//! through `Counting`, so what is reported is the live heap the chunk
//! store and the light map actually hold -- hash map buckets, `Arc`
//! headers and all -- rather than a sum of `size_of` that forgets the
//! container. Each figure is taken by building the structure, reading the
//! counter, dropping it and reading again: whatever the generator cached
//! along the way is in both readings and cancels.
//!
//! The world is the benchmark one (`saves/night`, seed 4242) at the
//! render distance the memory complaint was measured at, in the disc
//! `ChunkManager::inside` streams.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use primitive_shared::lighting::{compute_isolated, BlockSource, LightMap};
use primitive_shared::packed::PackedChunk;
use primitive_shared::types::{BlockId, Chunk, ChunkPos, BLOCK_AIR, CHUNK_SIZE_Y};
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

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

/// The client's chunk map as `ChunkManager` keeps it.
///
/// **Measured before packing, with this holding `Arc<Chunk>`** and the
/// light map holding a flat `Vec<u8>` per chunk:
///
/// ```text
/// blocks: 131 159 B/chunk, 224.3 MB
/// light:   65 611 B/chunk, 112.2 MB
/// total:  196 770 B/chunk, 336.5 MB
/// seam reconciliation: 0.078 ms/chunk
/// ```
struct Store(HashMap<ChunkPos, Arc<PackedChunk>>);

impl BlockSource for Store {
    fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
        if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
            return Some(BLOCK_AIR);
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        self.0.get(&pos).map(|c| c.get(lx, gy as usize, lz))
    }
    fn packed_chunk(&self, pos: ChunkPos) -> Option<&PackedChunk> {
        self.0.get(&pos).map(|c| &**c)
    }
}

const RADIUS: i32 = 24;
const MB: f64 = 1024.0 * 1024.0;

fn disc(centre: ChunkPos) -> Vec<ChunkPos> {
    let mut positions = Vec::new();
    for dx in -RADIUS..=RADIUS {
        for dz in -RADIUS..=RADIUS {
            if (dx.abs() <= 1 && dz.abs() <= 1) || dx * dx + dz * dz <= RADIUS * RADIUS {
                positions.push(ChunkPos::new(centre.x + dx, centre.z + dz));
            }
        }
    }
    // Nearest first, the order the client asks for and lights them in.
    positions.sort_by_key(|p| {
        let (dx, dz) = ((p.x - centre.x) as i64, (p.z - centre.z) as i64);
        dx * dx + dz * dz
    });
    positions
}

fn in_parallel<T: Send, R: Send>(items: Vec<T>, work: impl Fn(T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per = items.len().div_ceil(threads).max(1);
    let mut batches: Vec<Vec<T>> = Vec::new();
    let mut items = items.into_iter().peekable();
    while items.peek().is_some() {
        batches.push(items.by_ref().take(per).collect());
    }
    std::thread::scope(|scope| {
        let work = &work;
        let handles: Vec<_> = batches
            .into_iter()
            .map(|batch| scope.spawn(move || batch.into_iter().map(work).collect::<Vec<R>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

#[test]
#[ignore = "a measurement, not an assertion -- run it explicitly, in release"]
fn what_a_client_world_costs_in_memory() {
    let base = live();
    let gen = WorldGen::new(4242);
    let (sx, sz) = gen.spawn_column();
    let positions = disc(ChunkPos::from_global(sx, sz).0);
    let count = positions.len();

    let chunks: Vec<Chunk> = in_parallel(positions.clone(), |pos| gen.generate_chunk(pos));

    // --- the light map's input: the isolated pass, on workers, from the
    // flat blocks a worker decodes ---
    let isolated: Vec<(ChunkPos, Vec<u8>)> =
        in_parallel(chunks.iter().collect(), |chunk: &Chunk| (chunk.pos, compute_isolated(&chunk.blocks)));

    // --- the chunk store, packed the way the network thread packs ---
    let started = std::time::Instant::now();
    let packed: Vec<PackedChunk> = chunks.iter().map(PackedChunk::pack).collect();
    let packing = started.elapsed();
    drop(chunks);
    let store = Store(packed.into_iter().map(|c| (c.pos, Arc::new(c))).collect());
    let started = std::time::Instant::now();
    let decoded: usize = store.0.values().map(|c| c.to_blocks().len()).sum();
    let decoding = started.elapsed();
    assert_eq!(decoded, count * primitive_shared::types::CHUNK_VOLUME);

    // --- the light map, the seams reconciled in arrival order ---
    let mut light = LightMap::new();
    let started = std::time::Instant::now();
    for (pos, data) in isolated {
        light.insert_precomputed(&store, pos, data);
    }
    let reconcile = started.elapsed();

    // Each structure is weighed by what dropping it gives back. Its
    // buffers were allocated before it existed -- by the generator, by
    // the lighting pass -- and only *moved* in, so "the counter before
    // and after building it" would weigh the hash map and nothing else.
    let with_light = live();
    drop(light);
    let light_bytes = with_light - live();
    let with_store = live();
    drop(store);
    let store_bytes = with_store - live();
    // ...and whatever is left is what the generator kept for itself.
    let generator_bytes = live() - base;

    println!("{count} chunks, render distance {RADIUS}, seed 4242");
    println!(
        "blocks: {:.0} B/chunk, {:.1} MB",
        store_bytes as f64 / count as f64,
        store_bytes as f64 / MB
    );
    println!(
        "light:  {:.0} B/chunk, {:.1} MB",
        light_bytes as f64 / count as f64,
        light_bytes as f64 / MB
    );
    println!(
        "total:  {:.0} B/chunk, {:.1} MB",
        (store_bytes + light_bytes) as f64 / count as f64,
        (store_bytes + light_bytes) as f64 / MB
    );
    println!(
        "seam reconciliation: {:.3} ms/chunk",
        reconcile.as_secs_f64() * 1000.0 / count as f64
    );
    println!(
        "packing {:.3} ms/chunk, decoding {:.3} ms/chunk, one thread",
        packing.as_secs_f64() * 1000.0 / count as f64,
        decoding.as_secs_f64() * 1000.0 / count as f64
    );
    println!(
        "the generator kept {:.1} MB of memo after {count} chunks",
        generator_bytes as f64 / MB
    );
}
