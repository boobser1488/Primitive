//! One vertex buffer and one index buffer for the whole terrain.
//!
//! ## Why
//!
//! A chunk used to own two GPU buffers, and drawing the world meant, per
//! visible chunk, `set_vertex_buffer` + `set_index_buffer` + one draw.
//! At a render distance of eight that is around 250 visible chunks and
//! so around 750 calls into wgpu, and measurement put the whole of a
//! frame's terrain at roughly 0.6 ms -- about two thirds of it in the
//! two binds rather than in the draws.
//!
//! Binds are per-buffer, not per-draw. With every chunk's geometry
//! living at some offset inside *one* pair of buffers, the pass binds
//! once and each chunk costs exactly one `draw_indexed`: the index range
//! says which of the shared indices to read, and `base_vertex` shifts
//! them onto that chunk's own vertices, which is what lets the mesher go
//! on writing indices from zero as if it owned the buffer.
//!
//! ## The allocator
//!
//! First fit over a sorted free list, with adjacent free blocks merged
//! back together. That is the plainest allocator that does not fragment
//! itself to death, and it fits the access pattern: chunks are freed in
//! roughly the order they were made (the player walks away from what
//! they walked into), so freed space is usually adjacent to more freed
//! space.
//!
//! Sizes are in *elements* -- vertices and indices -- not bytes, because
//! that is what `base_vertex` and an index range are counted in, and a
//! unit conversion in the middle of an allocator is a bug waiting for a
//! Tuesday.
//!
//! ## Growing
//!
//! When nothing fits, the buffer doubles: a new one is made, the old
//! contents are copied across on the GPU, and the new tail joins the
//! free list. **Every existing offset stays valid**, which is the whole
//! reason to grow by copying rather than by repacking -- no live
//! allocation has to be found and rewritten, so no `GpuMesh` handed out
//! earlier can be left pointing at the wrong place.

/// A run of free elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    start: u64,
    len: u64,
}

/// Free space in one buffer, sorted by offset.
#[derive(Default)]
struct FreeList {
    runs: Vec<Run>,
}

impl FreeList {
    /// Takes `len` elements, or reports that nothing is big enough.
    fn alloc(&mut self, len: u64) -> Option<u64> {
        if len == 0 {
            return Some(0);
        }
        let index = self.runs.iter().position(|run| run.len >= len)?;
        let start = self.runs[index].start;
        if self.runs[index].len == len {
            self.runs.remove(index);
        } else {
            self.runs[index].start += len;
            self.runs[index].len -= len;
        }
        Some(start)
    }

    /// Gives a run back, merging it with whatever it now touches.
    ///
    /// Merging is what keeps this usable over a long session: without
    /// it, a world walked across for an hour leaves the free list as a
    /// few thousand chunk-sized holes, none of which fits the next
    /// chunk, and the buffer grows forever with most of it free.
    fn free(&mut self, start: u64, len: u64) {
        if len == 0 {
            return;
        }
        let index = self.runs.partition_point(|run| run.start < start);
        self.runs.insert(index, Run { start, len });

        // Merge forwards first, so the backward merge sees the whole
        // combined run and one `free` can never leave two adjacent
        // entries behind.
        if index + 1 < self.runs.len()
            && self.runs[index].start + self.runs[index].len == self.runs[index + 1].start
        {
            self.runs[index].len += self.runs[index + 1].len;
            self.runs.remove(index + 1);
        }
        if index > 0
            && self.runs[index - 1].start + self.runs[index - 1].len == self.runs[index].start
        {
            self.runs[index - 1].len += self.runs[index].len;
            self.runs.remove(index);
        }
    }

    /// Adds newly created space at the end of the buffer.
    fn extend(&mut self, start: u64, len: u64) {
        self.free(start, len);
    }

    #[cfg(test)]
    fn total_free(&self) -> u64 {
        self.runs.iter().map(|run| run.len).sum()
    }

    #[cfg(test)]
    fn holes(&self) -> usize {
        self.runs.len()
    }
}

/// Where one chunk's geometry lives inside the shared buffers.
///
/// `capacity` rather than the used length, because that is what has to
/// go back to the free list: a mesh is written into the block it was
/// given, and a shorter mesh next time does not shrink the block.
#[derive(Debug, Clone, Copy, Default)]
pub struct Block {
    pub start: u64,
    pub capacity: u64,
}

/// The shared buffers, plus who owns which part of them.
pub struct Arena {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    vertex_capacity: u64,
    index_capacity: u64,
    vertex_free: FreeList,
    index_free: FreeList,
    vertex_stride: u64,
    /// Space given back, waiting out the frames the GPU might still be
    /// reading it in. See `free`.
    quarantine: Vec<(u64, Block, Block)>,
    frame: u64,
}

/// How many frames a freed block waits before it can be handed out
/// again.
///
/// The renderer lets the CPU run up to two frames ahead of the GPU
/// (`desired_maximum_frame_latency`), so a chunk unloaded now may still
/// be referenced by command buffers that have not finished executing.
/// Writing another chunk's vertices over it while that is true puts one
/// chunk's geometry at another chunk's coordinates -- terrain stretched
/// into long leaning slabs, with the textures and lighting still
/// perfectly correct, because only the positions came from somewhere
/// else.
///
/// Three rather than two: the cost of being wrong is a rendering fault
/// that appears once a minute and cannot be reproduced on demand, and
/// the cost of one extra frame is that a few tens of kilobytes stay
/// reserved for another sixteenth of a second.
const QUARANTINE_FRAMES: u64 = 3;

/// Room for a world at a middling render distance before the first
/// growth: 2M vertices is 32 MB, and a chunk averages a few thousand.
///
/// Sized to make growth rare rather than impossible. Growing is correct
/// but it is not free -- it copies the whole buffer and waits for the
/// GPU twice -- and the cheapest growth is the one that never happens.
const INITIAL_VERTICES: u64 = 1 << 21;
const INITIAL_INDICES: u64 = 1 << 22;

impl Arena {
    pub fn new(device: &wgpu::Device, vertex_stride: u64) -> Self {
        Self::with_capacity(device, vertex_stride, INITIAL_VERTICES, INITIAL_INDICES)
    }

    /// The same, sized explicitly. Public so a test can start it small
    /// enough that growing is the *first* thing that happens -- see
    /// `tests/arena_growth.rs`, which exists because the growth path is
    /// where this got it wrong.
    pub fn with_capacity(
        device: &wgpu::Device,
        vertex_stride: u64,
        vertices: u64,
        indices: u64,
    ) -> Self {
        let vertex_buffer = make_buffer(
            device,
            "terrain vertex arena",
            vertices * vertex_stride,
            wgpu::BufferUsages::VERTEX,
        );
        let index_buffer = make_buffer(
            device,
            "terrain index arena",
            indices * 4,
            wgpu::BufferUsages::INDEX,
        );
        let mut vertex_free = FreeList::default();
        let mut index_free = FreeList::default();
        vertex_free.extend(0, vertices);
        index_free.extend(0, indices);
        Self {
            vertex_buffer,
            index_buffer,
            vertex_capacity: vertices,
            index_capacity: indices,
            vertex_free,
            index_free,
            vertex_stride,
            quarantine: Vec::new(),
            frame: 0,
        }
    }

    /// Call once per frame, before anything is uploaded.
    ///
    /// Releases whatever has waited long enough. Everything else stays
    /// reserved -- see `QUARANTINE_FRAMES`.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        let mut released = std::mem::take(&mut self.quarantine);
        released.retain(|&(freed_on, vertices, indices)| {
            if frame.saturating_sub(freed_on) < QUARANTINE_FRAMES {
                return true;
            }
            self.vertex_free.free(vertices.start, vertices.capacity);
            self.index_free.free(indices.start, indices.capacity);
            false
        });
        self.quarantine = released;
    }

    /// How many vertices and indices the arena currently holds room
    /// for. Used by the growth tests, which have to be sure the growth
    /// they are checking actually happened.
    #[cfg(test)]
    pub fn capacity(&self) -> (u64, u64) {
        (self.vertex_capacity, self.index_capacity)
    }

    /// Writes one chunk's geometry in, returning where it went.
    ///
    /// Grows either buffer if it has to, which is why this needs the
    /// queue and the device rather than just the queue.
    pub fn upload<V: bytemuck::Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[V],
        indices: &[u32],
    ) -> (Block, Block) {
        let vertex_block = self.claim_vertices(device, queue, vertices.len() as u64);
        let index_block = self.claim_indices(device, queue, indices.len() as u64);

        if !vertices.is_empty() {
            queue.write_buffer(
                &self.vertex_buffer,
                vertex_block.start * self.vertex_stride,
                bytemuck::cast_slice(vertices),
            );
        }
        if !indices.is_empty() {
            queue.write_buffer(
                &self.index_buffer,
                index_block.start * 4,
                bytemuck::cast_slice(indices),
            );
        }
        (vertex_block, index_block)
    }

    /// Hands a chunk's space back -- in a few frames' time.
    ///
    /// Not immediately, because "this chunk is no longer drawn" is a
    /// statement about the frame being built, not about the ones the GPU
    /// is still working through. See `QUARANTINE_FRAMES`.
    pub fn free(&mut self, vertex_block: Block, index_block: Block) {
        if vertex_block.capacity == 0 && index_block.capacity == 0 {
            return;
        }
        self.quarantine.push((self.frame, vertex_block, index_block));
    }

    /// Gives everything back at once, for a session ending.
    ///
    /// Safe without the wait because the caller is dropping every mesh
    /// in the world: nothing that could still be in flight will be drawn
    /// again, and the next frame starts from an empty map.
    pub fn release_all(&mut self) {
        for (_, vertices, indices) in std::mem::take(&mut self.quarantine) {
            self.vertex_free.free(vertices.start, vertices.capacity);
            self.index_free.free(indices.start, indices.capacity);
        }
    }

    fn claim_vertices(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        len: u64,
    ) -> Block {
        loop {
            if let Some(start) = self.vertex_free.alloc(len) {
                return Block { start, capacity: len };
            }
            let old = self.vertex_capacity;
            let wanted = (old * 2).max(old + len);
            self.vertex_buffer = grow(
                device,
                queue,
                &self.vertex_buffer,
                "terrain vertex arena",
                old * self.vertex_stride,
                wanted * self.vertex_stride,
                wgpu::BufferUsages::VERTEX,
            );
            self.vertex_free.extend(old, wanted - old);
            self.vertex_capacity = wanted;
        }
    }

    fn claim_indices(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, len: u64) -> Block {
        loop {
            if let Some(start) = self.index_free.alloc(len) {
                return Block { start, capacity: len };
            }
            let old = self.index_capacity;
            let wanted = (old * 2).max(old + len);
            self.index_buffer = grow(
                device,
                queue,
                &self.index_buffer,
                "terrain index arena",
                old * 4,
                wanted * 4,
                wgpu::BufferUsages::INDEX,
            );
            self.index_free.extend(old, wanted - old);
            self.index_capacity = wanted;
        }
    }
}

fn make_buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        // COPY_SRC as well as COPY_DST: growing copies the old buffer
        // into the new one, and the old one is the source.
        usage: usage | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

/// A bigger buffer with the old contents at the same offsets.
fn grow(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    old: &wgpu::Buffer,
    label: &str,
    old_bytes: u64,
    new_bytes: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    println!("terrain arena grew: {label} {old_bytes} -> {new_bytes} bytes");

    // Everything already written this frame goes into the old buffer
    // *first*.
    //
    // `write_buffer` does not write anything: it stages the data against
    // the buffer it was given and applies it at the next submit. Growing
    // replaces that buffer, so any chunk uploaded earlier in this frame
    // has its data staged against a buffer that is about to be thrown
    // away -- the copy below carries across a region those chunks were
    // never written into, and what the GPU then draws from their offsets
    // is whatever the driver left there. On screen that is terrain
    // stretched into long leaning slabs, because uninitialised bytes
    // read as arbitrary float positions.
    //
    // An empty *command buffer*, not an empty submit: a submit with
    // nothing in it is entitled to do nothing at all, staged writes
    // included, and the first attempt at this fix -- which passed an
    // empty iterator -- left the corruption exactly where it was.
    // Waiting afterwards removes the last of the doubt: by the time
    // `poll` returns, everything queued against the old buffer has
    // actually happened.
    //
    // Both cost real time, and both are paid two or three times in the
    // life of a world rather than per frame.
    let flush = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("arena flush"),
    });
    queue.submit(std::iter::once(flush.finish()));
    device.poll(wgpu::Maintain::Wait);

    let fresh = make_buffer(device, label, new_bytes, usage);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("arena growth"),
    });
    encoder.copy_buffer_to_buffer(old, 0, &fresh, 0, old_bytes);
    queue.submit(std::iter::once(encoder.finish()));
    // ...and the copy has to be finished before anything is written
    // into the new buffer, or the copy would land on top of it.
    device.poll(wgpu::Maintain::Wait);
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(capacity: u64) -> FreeList {
        let mut free = FreeList::default();
        free.extend(0, capacity);
        free
    }

    #[test]
    fn allocations_do_not_overlap() {
        let mut free = list(100);
        let a = free.alloc(30).unwrap();
        let b = free.alloc(30).unwrap();
        let c = free.alloc(40).unwrap();
        let mut spans = [(a, 30), (b, 30), (c, 40)];
        spans.sort();
        assert_eq!(spans, [(0, 30), (30, 30), (60, 40)]);
        assert_eq!(free.total_free(), 0);
        assert!(free.alloc(1).is_none(), "handed out space it did not have");
    }

    #[test]
    fn freed_neighbours_merge_back_into_one_run() {
        // Without merging, an hour of walking leaves a few thousand
        // chunk-sized holes, none of which fits the next chunk, and the
        // buffer grows forever with most of it free.
        let mut free = list(300);
        let a = free.alloc(100).unwrap();
        let b = free.alloc(100).unwrap();
        let c = free.alloc(100).unwrap();
        assert_eq!(free.holes(), 0);

        free.free(a, 100);
        free.free(c, 100);
        assert_eq!(free.holes(), 2, "two separate holes with b still held");
        // Returning the middle one has to close both gaps at once.
        free.free(b, 100);
        assert_eq!(free.holes(), 1, "the three runs did not merge");
        assert_eq!(free.total_free(), 300);
        assert_eq!(free.alloc(300), Some(0), "the whole buffer is one run again");
    }

    #[test]
    fn a_hole_is_reused_before_the_buffer_is_asked_to_grow() {
        let mut free = list(100);
        let a = free.alloc(40).unwrap();
        let _b = free.alloc(40).unwrap();
        free.free(a, 40);
        // First fit: the reopened hole at the front, not the tail.
        assert_eq!(free.alloc(40), Some(a));
    }

    #[test]
    fn a_run_too_small_is_skipped_rather_than_split_wrongly() {
        let mut free = list(100);
        let a = free.alloc(10).unwrap();
        let b = free.alloc(50).unwrap();
        free.free(a, 10); // a ten-element hole at the front
        let big = free.alloc(30).unwrap();
        assert_ne!(big, a, "a 30 went into a hole of 10");
        assert_eq!(big, b + 50);
    }

    #[test]
    fn nothing_is_allocated_for_an_empty_mesh() {
        // A sky chunk has no geometry, and it must not consume a run --
        // nor may freeing its zero-size block corrupt the list.
        let mut free = list(100);
        assert_eq!(free.alloc(0), Some(0));
        assert_eq!(free.total_free(), 100);
        free.free(0, 0);
        assert_eq!(free.holes(), 1);
        assert_eq!(free.total_free(), 100);
    }

    #[test]
    fn growth_leaves_every_existing_offset_where_it_was() {
        // The reason growth copies rather than repacks: a `GpuMesh`
        // handed out earlier still points at its own geometry.
        let mut free = list(100);
        let a = free.alloc(60).unwrap();
        let b = free.alloc(40).unwrap();
        assert!(free.alloc(10).is_none());
        free.extend(100, 100); // the new tail after doubling
        let c = free.alloc(10).unwrap();
        assert_eq!((a, b), (0, 60), "existing allocations moved");
        assert_eq!(c, 100, "the new space is past the old end");
    }

    #[test]
    fn a_long_session_of_churn_stays_bounded() {
        // The property that matters: allocate and free in the order a
        // player walking a straight line would, and the list must not
        // grind down into unusable dust.
        let mut free = list(10_000);
        let mut live: Vec<(u64, u64)> = Vec::new();
        for step in 0..2_000u64 {
            let size = 40 + step % 60;
            if let Some(start) = free.alloc(size) {
                live.push((start, size));
            }
            if live.len() > 50 {
                let (start, size) = live.remove(0);
                free.free(start, size);
            }
        }
        assert!(
            free.holes() < 64,
            "the free list fragmented into {} holes",
            free.holes()
        );
    }
}

/// The arena against a real GPU.
///
/// Everything above can be unit-tested except the one thing that went
/// wrong, which was not arithmetic: `Queue::write_buffer` does not write
/// anything, it *stages* the data against the buffer it was handed and
/// applies it at the next submit. Growing replaces that buffer, so every
/// chunk uploaded earlier in the same frame had its data staged against
/// a buffer that was about to be thrown away, and the copy into the new
/// one carried across a region those chunks had never been written into.
///
/// What the player saw was terrain stretched into long leaning slabs:
/// the block those vertices sat in still held the *previous* occupant's
/// geometry, which is why the textures and the lighting looked right and
/// only the positions were wrong. The free-list arithmetic was correct
/// throughout, and no amount of testing it would have found this.
///
/// So these do the only thing that could: write real data, force a
/// growth, read the buffer back off the GPU and check that what went in
/// is still there.
///
/// They need an adapter. On a machine without one -- a CI runner with no
/// GPU -- they say so and pass, because the alternative is a suite that
/// cannot be run at all in the place it is most wanted.
#[cfg(test)]
mod gpu_tests {
    use super::Arena;


    /// A device, or nothing if this machine has no GPU to test against.
    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))?;
        pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("arena test device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        ))
        .ok()
    }

    /// Reads `count` u32s back from a buffer.
    fn read_back(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffer: &wgpu::Buffer,
        offset: u64,
        count: usize,
    ) -> Vec<u32> {
        let bytes = (count * 4) as u64;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(buffer, offset, &staging, 0, bytes);
        queue.submit(std::iter::once(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().expect("map never completed").expect("map failed");
        let data = slice.get_mapped_range();
        let out: Vec<u32> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        staging.unmap();
        out
    }

    #[test]
    fn what_was_written_before_a_growth_is_still_there_after_it() {
        let Some((device, queue)) = gpu() else {
            println!("no GPU adapter on this machine; skipping the arena growth test");
            return;
        };

        // Deliberately too small to hold both uploads, so the second one
        // has to grow the buffers. Four bytes a "vertex" keeps the
        // arithmetic in the test obvious.
        let mut arena = Arena::with_capacity(&device, 4, 64, 64);

        // Two uploads in a row with no submit between them -- which is
        // exactly what a frame of streaming terrain does, and exactly the
        // case that used to lose the first one.
        let first: Vec<u32> = (0..40).map(|i| 0xA000_0000 + i).collect();
        let second: Vec<u32> = (0..40).map(|i| 0xB000_0000 + i).collect();

        let before = arena.capacity();
        let (a_vertices, a_indices) = arena.upload(&device, &queue, &first, &first);
        let (b_vertices, b_indices) = arena.upload(&device, &queue, &second, &second);

        assert!(
            arena.capacity().0 > before.0 && arena.capacity().1 > before.1,
            "the second upload did not grow the arena, so this proves nothing"
        );
        assert_ne!(
            a_vertices.start, b_vertices.start,
            "two uploads landed in the same place"
        );

        let read_a = read_back(&device, &queue, &arena.vertex_buffer, a_vertices.start * 4, 40);
        assert_eq!(
            read_a, first,
            "the first upload was lost when the arena grew under it"
        );
        let read_b = read_back(&device, &queue, &arena.vertex_buffer, b_vertices.start * 4, 40);
        assert_eq!(read_b, second, "the upload that caused the growth was lost");

        // ...and the same for the index buffer, which grows independently.
        let read_a = read_back(&device, &queue, &arena.index_buffer, a_indices.start * 4, 40);
        assert_eq!(read_a, first, "the first upload's indices were lost");
        let read_b = read_back(&device, &queue, &arena.index_buffer, b_indices.start * 4, 40);
        assert_eq!(read_b, second, "the growing upload's indices were lost");
    }

    #[test]
    fn a_run_of_uploads_across_several_growths_all_survive() {
        let Some((device, queue)) = gpu() else {
            println!("no GPU adapter on this machine; skipping the arena growth test");
            return;
        };

        // Small enough that this grows repeatedly, the way a world streaming
        // in for the first time does.
        let mut arena = Arena::with_capacity(&device, 4, 32, 32);
        let mut placed = Vec::new();
        for chunk in 0..24u32 {
            let data: Vec<u32> = (0..16).map(|i| (chunk << 16) | i).collect();
            let (vertices, _) = arena.upload(&device, &queue, &data, &data);
            placed.push((vertices.start, data));
        }

        for (start, expected) in placed {
            let got = read_back(&device, &queue, &arena.vertex_buffer, start * 4, expected.len());
            assert_eq!(got, expected, "an upload at {start} did not survive");
        }
    }
}
