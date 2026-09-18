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
//! When nothing fits, the buffer grows by half again (see
//! `grown_capacity` for why not double): a new one is made, the old
//! contents are copied across on the GPU, and the new tail joins the
//! free list. **Every existing offset stays valid**, which is the whole
//! reason to grow by copying rather than by repacking -- no live
//! allocation has to be found and rewritten, so no `GpuMesh` handed out
//! earlier can be left pointing at the wrong place.
//!
//! ## The ceiling
//!
//! **A buffer has a largest legal size, and doubling past it is a crash
//! rather than an error.** A player at render distance 24 filled the
//! vertex arena to 167 MB, the next doubling asked the driver for 320,
//! and the game died before the frame:
//!
//! ```text
//! In Device::create_buffer, label = `terrain vertex arena`
//! Buffer size 335544320 is greater than the maximum buffer size (268435456)
//! ```
//!
//! That 256 MB was not the card's limit. It is `wgpu::Limits::default()`,
//! which the renderer was asking for verbatim; the GTX 1050 Ti this
//! happened on offers gigabytes. So the first half of the fix is in
//! `GraphicsState::new`, which now asks for the *adapter's*
//! `max_buffer_size`, and the second half is here: the arena is told what
//! that number turned out to be and never grows past it. Where the real
//! ceiling is genuinely reached, an upload is **refused** -- the chunk
//! goes undrawn and says so once -- because a hole in the far terrain is
//! a thing a player can walk out of, and a panic is not.

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
    /// The largest buffer this device will make, in bytes. See the
    /// module header: growing past it is a validation panic, so it is
    /// carried here rather than looked up at the point of despair.
    max_bytes: u64,
    /// Whether a chunk has already been turned away for want of room.
    /// One line per session, not one per frame: a full arena refuses
    /// every chunk the player walks toward.
    refused: bool,
    /// Space given back, waiting out the frames the GPU might still be
    /// reading it in. See `free`.
    quarantine: Vec<(u64, Block, Block)>,
    frame: u64,
}

/// How big a buffer to ask for next, or `None` where the request cannot
/// be served at all.
///
/// Half again, then whatever the request needs, then the ceiling -- in
/// that order, so the ceiling wins. Everything is a *capacity in
/// elements*: `needed` is the total the buffer must hold (the caller
/// asks for `old + len`, since the free space it could not use may be
/// scattered), and `cap_elements` is the device's largest buffer in the
/// same unit, which is the caller's job to convert because vertices and
/// indices are different sizes.
///
/// `None` means the ceiling is real and this mesh does not fit under it.
/// It is not a failure to grow -- there is nowhere to grow to -- and the
/// caller turns the chunk away rather than asking again next frame.
///
/// **Half again rather than double, and it was double until the world
/// was measured.** A growth copies the whole buffer and waits for the
/// GPU twice, so the factor trades video memory against how many of
/// those a session pays for -- and doubling was spending the memory. The
/// benchmark world at render distance 24 (`world_cost` below) streams
/// 119 MB of vertices and 40 MB of indices into the arena, and doubling
/// had grown the buffers to 160 and 64 by then: **224 MB on the card for
/// 159 MB of terrain**, the difference allocated and never drawn from.
/// Half again takes the same world to 135 and 54, **189 MB**, for one
/// more growth of each buffer -- three rather than two, once per
/// session, on a stream that is already landing a chunk at a time.
///
/// A quarter was worked through the same simulation and not taken: 122
/// and 49, 171 MB, for **five** growths of each buffer. That is eighteen
/// megabytes bought with four more copies of the biggest buffers in the
/// game, each with two waits on the GPU, and the eighteen are luck --
/// what a finer step saves depends on where this particular world's
/// terrain stops against the steps, while the extra copies are paid
/// every time.
fn grown_capacity(old: u64, needed: u64, cap_elements: u64) -> Option<u64> {
    let wanted = (old + old / 2).max(needed).min(cap_elements);
    // Strictly bigger: a "growth" to the size it already is would spend
    // a buffer copy and two GPU waits to change nothing, and then be
    // asked for again on the very next chunk.
    (wanted > old && wanted >= needed).then_some(wanted)
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

/// The largest either buffer is allowed to become, whatever the driver
/// says it would accept.
///
/// A gigabyte. **The card's own answer turned out to be no answer at
/// all**: the GTX 1050 Ti this was measured on reports a
/// `max_buffer_size` of about sixteen petabytes -- the limit is a
/// formality in Vulkan, and the real wall is the four gigabytes of video
/// memory, which no limit mentions. Doubling toward that wall would end
/// in an allocation failure inside the driver, which is a crash with a
/// worse message than the one this replaced.
///
/// A gigabyte is eight times what a render distance of 24 actually uses
/// (measured: 167 MB of vertices), so nothing a player is likely to set
/// reaches it -- and a world that does gets chunks refused with a line
/// saying so, which is a thing to read rather than a thing to debug.
const MAX_ARENA_BYTES: u64 = 1 << 30;

/// The ceiling in force on this device: ours, or the driver's if it is
/// somehow lower. Public so the startup line and the arena cannot
/// disagree about the number.
pub fn ceiling(device: &wgpu::Device) -> u64 {
    device.limits().max_buffer_size.min(MAX_ARENA_BYTES)
}

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
        // Asked of the device that will make the buffers, not of the
        // adapter and not written down: `GraphicsState::new` raises the
        // driver's limit where the card allows, and `MAX_ARENA_BYTES`
        // is what we are willing to spend of it.
        let max_bytes = ceiling(device);
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
            max_bytes,
            refused: false,
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

    /// Bytes of terrain the buffers hold, and bytes the buffers are:
    /// `(used, allocated)`, both buffers together.
    ///
    /// **For the F3 line, so the card's share can be read off a running
    /// game.** The gap between the two is what growing costs (see
    /// `grown_capacity`), and it was invisible: the only arena number the
    /// game ever printed was the size of a growth, once, at the moment
    /// it happened. Space waiting out its quarantine counts as used,
    /// which it is for those three frames.
    pub fn usage(&self) -> (u64, u64) {
        let used = (self.vertex_capacity - self.vertex_free.total_free()) * self.vertex_stride
            + (self.index_capacity - self.index_free.total_free()) * 4;
        let allocated = self.vertex_capacity * self.vertex_stride + self.index_capacity * 4;
        (used, allocated)
    }

    /// Writes one chunk's geometry in, returning where it went.
    ///
    /// Grows either buffer if it has to, which is why this needs the
    /// queue and the device rather than just the queue.
    ///
    /// `None` where the device's largest buffer is already in use and
    /// this mesh does not fit in what is free: the chunk is not drawn,
    /// and the caller keeps whatever it had before rather than a
    /// half-written mesh. See the module header.
    pub fn upload<V: bytemuck::Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &[V],
        indices: &[u32],
    ) -> Option<(Block, Block)> {
        let vertex_block = self.claim_vertices(device, queue, vertices.len() as u64)?;
        let Some(index_block) = self.claim_indices(device, queue, indices.len() as u64) else {
            // The vertices were claimed and nothing will be written into
            // them: hand them straight back, or the arena leaks a
            // chunk's worth of space every time this happens.
            self.free(vertex_block, Block::default());
            return None;
        };

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
        Some((vertex_block, index_block))
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
    ) -> Option<Block> {
        loop {
            if let Some(start) = self.vertex_free.alloc(len) {
                return Some(Block { start, capacity: len });
            }
            let old = self.vertex_capacity;
            let Some(wanted) = grown_capacity(old, old + len, self.max_bytes / self.vertex_stride)
            else {
                self.refuse("terrain vertex arena", old * self.vertex_stride);
                return None;
            };
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

    fn claim_indices(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        len: u64,
    ) -> Option<Block> {
        loop {
            if let Some(start) = self.index_free.alloc(len) {
                return Some(Block { start, capacity: len });
            }
            let old = self.index_capacity;
            let Some(wanted) = grown_capacity(old, old + len, self.max_bytes / 4) else {
                self.refuse("terrain index arena", old * 4);
                return None;
            };
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

    /// Says once that the world is bigger than the card's largest
    /// buffer, and names the lever: this is a render-distance problem,
    /// and a player who is told that can fix it in ten seconds.
    fn refuse(&mut self, label: &str, bytes: u64) {
        if self.refused {
            return;
        }
        self.refused = true;
        println!(
            "{label} is full at {} MB, which is this device's largest buffer -- \
             far chunks will not be drawn. Lower the render distance.",
            bytes / (1024 * 1024)
        );
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

    /// **The arena stops at the device's largest buffer instead of
    /// asking for one it cannot have.** This is the arithmetic behind
    /// the crash in the module header: a full 167 MB arena doubling to
    /// 320 against a 256 MB ceiling. Doubling is kept while it fits,
    /// the ceiling is taken exactly when the double would overshoot it,
    /// and once the buffer *is* the ceiling there is no growth left to
    /// report -- which is the answer the caller turns into a refused
    /// chunk rather than a panic.
    #[test]
    fn the_arena_grows_up_to_the_devices_largest_buffer_and_then_says_no() {
        // Ordinary growth, well under the ceiling: half again.
        assert_eq!(grown_capacity(64, 65, 4096), Some(96));
        // A mesh bigger than the double: take what it needs.
        assert_eq!(grown_capacity(64, 900, 4096), Some(900));
        // The double overshoots the ceiling, so the ceiling it is -- and
        // this is still growth, so the upload goes through.
        assert_eq!(grown_capacity(3000, 3001, 4096), Some(4096));
        // At the ceiling with room for the mesh nowhere: no growth, and
        // no request the driver would refuse.
        assert_eq!(grown_capacity(4096, 4097, 4096), None);
        // ...and a mesh larger than the whole ceiling is refused rather
        // than served a buffer it does not fit in.
        assert_eq!(grown_capacity(64, 9000, 4096), None);
    }

    /// **A buffer that has just grown is never more than a third empty.**
    /// This is the property doubling did not have -- right after a
    /// doubling half the buffer is unused, and on the benchmark world it
    /// was 65 MB of the card's memory holding nothing (see
    /// `grown_capacity`). Checked over every size a growth can be asked
    /// for, so that a change to the factor that brings the waste back
    /// fails here rather than in a measurement nobody reruns.
    #[test]
    fn a_buffer_that_has_just_grown_is_never_more_than_a_third_empty() {
        for old in [2u64, 3, 64, 1000, 1 << 21] {
            for extra in [1u64, 2, old / 3, old / 2, old, old * 3] {
                let needed = old + extra.max(1);
                let Some(wanted) = grown_capacity(old, needed, u64::MAX) else {
                    panic!("no growth from {old} for {needed}");
                };
                assert!(wanted >= needed, "grew {old} to {wanted}, short of {needed}");
                assert!(
                    (wanted - needed) * 3 <= wanted,
                    "grew {old} to {wanted} for {needed}: {} unused",
                    wanted - needed
                );
            }
        }
    }


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

/// What a real world asks of the arena, measured without a GPU.
///
/// ```text
/// cargo test --release -p primitive_client --lib what_a_world_asks_of_the_arena \
///     -- --ignored --nocapture
/// ```
///
/// **The arena's size is a fact about the terrain, not about the card**,
/// so it can be measured where the terrain is: the benchmark world
/// (`saves/night`, seed 4242) at render distance 24, streamed as the
/// client streams it -- nearest first, lit through the same `LightMap`,
/// levelled by the same `lod::level_at` with the stock settings, and
/// meshed by the real `build_mesh`. The layers are `numbered_for_test`,
/// one picture per block, which merges a little *less* than the real
/// atlas (where some blocks share a file): an upper bound rather than a
/// flattering one.
///
/// What a card would hold is then two numbers: what the meshes use, and
/// what the doubling in `claim_vertices` would have grown the buffers to
/// by the time the last of them went in. The gap between those two is
/// memory that is allocated and never drawn from.
#[cfg(test)]
mod world_cost {
    use super::{grown_capacity, INITIAL_INDICES, INITIAL_VERTICES, MAX_ARENA_BYTES};
    use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood, Vertex};
    use crate::logic::chunk_manager::ChunkManager;
    use primitive_shared::lighting::{compute_isolated, BlockSource, LightMap};
    use primitive_shared::types::{BlockId, Chunk, ChunkPos, BLOCK_AIR, CHUNK_SIZE_Y};
    use primitive_shared::worldgen::WorldGen;

    const RADIUS: i32 = 24;
    const MB: f64 = 1024.0 * 1024.0;

    /// The same chunks as plain arrays, for timing `fill` against the
    /// flat path it took before chunks were kept packed.
    ///
    /// **In the same process, on the same terrain, interleaved with the
    /// packed fill**, because the first attempt compared two runs and the
    /// runs disagreed with themselves by a fifth: a release test binary
    /// runs while the build directory's lock is free, and whatever cargo
    /// was queued behind it compiles on the same cores.
    struct Flat(std::collections::HashMap<ChunkPos, Chunk>);

    impl BlockSource for Flat {
        fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
            if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
                return Some(BLOCK_AIR);
            }
            let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
            self.0.get(&pos).map(|chunk| chunk.get(lx, gy as usize, lz))
        }
        fn chunk_data(&self, pos: ChunkPos) -> Option<&[BlockId]> {
            self.0.get(&pos).map(|chunk| chunk.blocks.as_slice())
        }
    }

    fn in_parallel<T: Send + Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let per = items.len().div_ceil(threads).max(1);
        std::thread::scope(|scope| {
            let work = &work;
            let handles: Vec<_> = items
                .chunks(per)
                .map(|batch| scope.spawn(move || batch.iter().map(work).collect::<Vec<R>>()))
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
        })
    }

    /// Final capacity of one buffer after `sizes` are claimed in order
    /// with nothing freed, and how many growths it took: exactly the path
    /// `claim_vertices` takes while a world streams in for the first
    /// time.
    ///
    /// **Measured with doubling, before `grown_capacity` became half
    /// again:**
    ///
    /// ```text
    /// vertices: 6262274 x 20 B = 119.4 MB used, buffer grown to 160.0 MB
    /// indices:  10431786 x 4 B = 39.8 MB used, buffer grown to 64.0 MB
    /// arena: 159.2 MB used, 224.0 MB allocated
    /// largest chunk: 17262 vertices; 0 chunk(s) past a u16 index
    /// ```
    ///
    /// The timings are not in that list on purpose. Two runs of this
    /// test disagreed with each other by a fifth when cargo was compiling
    /// on the same cores, so `fill` is compared *within* one run instead,
    /// against the same chunks held flat -- see `Flat`.
    fn grown_to(initial: u64, sizes: impl Iterator<Item = u64>, cap_elements: u64) -> (u64, u32) {
        let (mut capacity, mut used, mut growths) = (initial, 0u64, 0u32);
        for len in sizes {
            while used + len > capacity {
                match grown_capacity(capacity, capacity + len, cap_elements) {
                    Some(wanted) => {
                        capacity = wanted;
                        growths += 1;
                    }
                    None => return (capacity, growths),
                }
            }
            used += len;
        }
        (capacity, growths)
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly, in release"]
    fn what_a_world_asks_of_the_arena() {
        let settings = crate::settings::ClientSettings::default();
        let generator = WorldGen::new(4242);
        let (sx, sz) = generator.spawn_column();
        let centre = ChunkPos::from_global(sx, sz).0;
        let chunks_manager_probe = ChunkManager::new(RADIUS);
        let mut positions = Vec::new();
        for dx in -RADIUS..=RADIUS {
            for dz in -RADIUS..=RADIUS {
                if chunks_manager_probe.inside(dx, dz) {
                    positions.push(ChunkPos::new(centre.x + dx, centre.z + dz));
                }
            }
        }
        let distance = |p: &ChunkPos| {
            let (dx, dz) = ((p.x - centre.x) as f32, (p.z - centre.z) as f32);
            (dx * dx + dz * dz).sqrt()
        };
        positions.sort_by(|a, b| distance(a).total_cmp(&distance(b)));

        let generated: Vec<Chunk> = in_parallel(&positions, |&pos| generator.generate_chunk(pos));
        let isolated: Vec<Vec<u8>> = in_parallel(&generated, |chunk| compute_isolated(&chunk.blocks));
        // Flat copies of the middle of the world only: the 64 nearest and
        // the ring they are filled against are all within six chunks.
        let flat = Flat(
            generated
                .iter()
                .filter(|chunk| distance(&chunk.pos) <= 6.0)
                .map(|chunk| (chunk.pos, chunk.clone()))
                .collect(),
        );
        let mut chunks = ChunkManager::new(RADIUS);
        for chunk in generated {
            chunks.insert(chunk);
        }
        let mut light = LightMap::new();
        for (pos, data) in positions.iter().zip(isolated) {
            light.insert_precomputed(&chunks, *pos, data);
        }

        let layers = crate::engine::texture::FaceLayers::numbered_for_test();
        let lod = settings.lod_distance_chunks;
        let quality = settings.lod_quality;
        let meshed: Vec<(u64, u64)> = in_parallel(&positions, |pos| {
            let mut cache = Box::<Neighbourhood>::default();
            let mut out = Box::<MeshBuffers>::default();
            cache.fill(*pos, &chunks, &light);
            let start = crate::engine::lod::band_start(lod, cache.ceiling());
            let level = crate::engine::lod::level_at(distance(pos), start, 0);
            crate::engine::lod::coarsen(&mut cache, level, quality);
            build_mesh(*pos, &cache, &layers, &generator, &mut out);
            (out.vertices.len() as u64, out.indices.len() as u64)
        });

        let stride = std::mem::size_of::<Vertex>() as u64;
        let vertices: u64 = meshed.iter().map(|m| m.0).sum();
        let indices: u64 = meshed.iter().map(|m| m.1).sum();
        let (vertex_capacity, vertex_growths) = grown_to(
            INITIAL_VERTICES,
            meshed.iter().map(|m| m.0),
            MAX_ARENA_BYTES / stride,
        );
        let (index_capacity, index_growths) =
            grown_to(INITIAL_INDICES, meshed.iter().map(|m| m.1), MAX_ARENA_BYTES / 4);
        let largest = meshed.iter().map(|m| m.0).max().unwrap_or(0);
        let over_u16 = meshed.iter().filter(|m| m.0 > u16::MAX as u64).count();

        println!(
            "{} chunks, render distance {RADIUS}, lod from {lod} chunks ({quality:?}), seed 4242",
            positions.len()
        );
        println!(
            "vertices: {vertices} x {stride} B = {:.1} MB used, buffer grown to {:.1} MB",
            (vertices * stride) as f64 / MB,
            (vertex_capacity * stride) as f64 / MB
        );
        println!(
            "indices:  {indices} x 4 B = {:.1} MB used, buffer grown to {:.1} MB",
            (indices * 4) as f64 / MB,
            (index_capacity * 4) as f64 / MB
        );
        println!(
            "arena: {:.1} MB used, {:.1} MB allocated, after {vertex_growths} vertex and \
             {index_growths} index growth(s)",
            (vertices * stride + indices * 4) as f64 / MB,
            (vertex_capacity * stride + index_capacity * 4) as f64 / MB
        );
        println!("largest chunk: {largest} vertices; {over_u16} chunk(s) past a u16 index");

        // Fill and mesh, one thread, the 64 nearest -- the figure the
        // height change was measured by.
        let mut best = (f64::MAX, f64::MAX, f64::MAX);
        let mut cache = Box::<Neighbourhood>::default();
        let mut out = Box::<MeshBuffers>::default();
        for _ in 0..7 {
            let (mut packed_fill, mut flat_fill, mut mesh) = (0.0, 0.0, 0.0);
            for pos in positions.iter().take(64) {
                let started = std::time::Instant::now();
                cache.fill(*pos, &flat, &light);
                flat_fill += started.elapsed().as_secs_f64();
                let started = std::time::Instant::now();
                cache.fill(*pos, &chunks, &light);
                packed_fill += started.elapsed().as_secs_f64();
                out.clear();
                let started = std::time::Instant::now();
                build_mesh(*pos, &cache, &layers, &generator, &mut out);
                mesh += started.elapsed().as_secs_f64();
            }
            best = (best.0.min(packed_fill), best.1.min(flat_fill), best.2.min(mesh));
        }
        println!(
            "64 nearest, one thread: fill {:.2} ms (the same chunks flat: {:.2} ms), build_mesh {:.1} ms",
            best.0 * 1000.0,
            best.1 * 1000.0,
            best.2 * 1000.0
        );
    }

    /// What a chunk costs to light, fill and mesh in each kind of country,
    /// and how much geometry it hands the GPU.
    ///
    /// ```text
    /// cargo test --release -p primitive_client --lib what_each_country_costs_to_mesh \
    ///     -- --ignored --nocapture
    /// ```
    ///
    /// **Every other tool in this module stands on the benchmark's temperate
    /// shore**, and the geometry that landed since -- palm trunks drawn a
    /// quarter-slice at a time, kelp and coral on the seabed, roots and
    /// standing water in the marsh -- lives on tropical beaches, under the sea
    /// and in the swamp. A mesher that got dearer only there reads as
    /// unchanged from the shore, so this one goes to each country and meshes
    /// the three-by-three at its middle, with a ring of real neighbours round
    /// it so the seams cull as they do in a streamed world.
    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly, in release"]
    fn what_each_country_costs_to_mesh() {
        use primitive_shared::worldgen::{Biome, Preset, Zone};
        let places = [
            ("temperate plains", Zone::Temperate, Biome::Plains),
            ("temperate forest", Zone::Temperate, Biome::Forest),
            ("temperate swamp", Zone::Temperate, Biome::Swamp),
            ("temperate sea", Zone::Temperate, Biome::Ocean),
            ("tropical beach", Zone::Tropics, Biome::Beach),
            ("tropical sea", Zone::Tropics, Biome::Ocean),
            ("dry-belt desert", Zone::DryBelt, Biome::Desert),
            ("northern bog", Zone::North, Biome::Bog),
        ];
        let layers = crate::engine::texture::FaceLayers::numbered_for_test();
        let mut cache = Box::<Neighbourhood>::default();
        let mut out = Box::<MeshBuffers>::default();
        for (name, zone, wanted) in places {
            let generator = WorldGen::with_zone(1234, Preset::Normal, zone);
            let mut nearest: Option<(i64, ChunkPos)> = None;
            for gz in (-3008..3008).step_by(64) {
                for gx in (-3008..3008).step_by(64) {
                    let distance = i64::from(gx) * i64::from(gx) + i64::from(gz) * i64::from(gz);
                    if nearest.is_some_and(|(best, _)| best <= distance) || generator.biome_at(gx + 8, gz + 8) != wanted {
                        continue;
                    }
                    nearest = Some((distance, ChunkPos::from_global(gx, gz).0));
                }
            }
            let Some((_, centre)) = nearest else {
                println!("[mesh] {name:17} none within three thousand blocks");
                continue;
            };
            let around: Vec<ChunkPos> = (-2..=2)
                .flat_map(|dz| (-2..=2).map(move |dx| ChunkPos::new(centre.x + dx, centre.z + dz)))
                .collect();
            let generated: Vec<Chunk> = around.iter().map(|&pos| generator.generate_chunk(pos)).collect();
            let mut best_light = f64::MAX;
            for _ in 0..3 {
                let started = std::time::Instant::now();
                for chunk in &generated {
                    std::hint::black_box(compute_isolated(&chunk.blocks));
                }
                best_light = best_light.min(started.elapsed().as_secs_f64() / generated.len() as f64);
            }
            let isolated: Vec<Vec<u8>> = generated.iter().map(|chunk| compute_isolated(&chunk.blocks)).collect();
            let mut chunks = ChunkManager::new(4);
            for chunk in generated {
                chunks.insert(chunk);
            }
            let mut light = LightMap::new();
            for (pos, data) in around.iter().zip(isolated) {
                light.insert_precomputed(&chunks, *pos, data);
            }
            let inner: Vec<ChunkPos> = around
                .iter()
                .copied()
                .filter(|pos| (pos.x - centre.x).abs() <= 1 && (pos.z - centre.z).abs() <= 1)
                .collect();
            let (mut best_fill, mut best_mesh, mut vertices, mut indices) = (f64::MAX, f64::MAX, 0usize, 0usize);
            for _ in 0..5 {
                let (mut fill, mut mesh) = (0.0, 0.0);
                (vertices, indices) = (0, 0);
                for pos in &inner {
                    let started = std::time::Instant::now();
                    cache.fill(*pos, &chunks, &light);
                    fill += started.elapsed().as_secs_f64();
                    out.clear();
                    let started = std::time::Instant::now();
                    build_mesh(*pos, &cache, &layers, &generator, &mut out);
                    mesh += started.elapsed().as_secs_f64();
                    vertices += out.vertices.len();
                    indices += out.indices.len();
                }
                best_fill = best_fill.min(fill);
                best_mesh = best_mesh.min(mesh);
            }
            let n = inner.len() as f64;
            println!(
                "[mesh] {name:17} light {:.3} | fill {:.3} | mesh {:.3} ms/chunk | {:.0} vertices, {:.0} triangles a chunk",
                best_light * 1e3,
                best_fill * 1e3 / n,
                best_mesh * 1e3 / n,
                vertices as f64 / n,
                indices as f64 / 3.0 / n
            );
        }
    }

    /// **Whose triangles the benchmark scene is made of**, by block kind and
    /// by pass: the fine-meshed ring (`lod_distance_chunks`, ten) of world
    /// `night` round the pinned benchmark spawn.
    ///
    /// ```text
    /// cargo test --release -p primitive_client --lib what_the_benchmark_scene_is_made_of \
    ///     -- --ignored --nocapture
    /// ```
    ///
    /// The frame is bound by the solid pass and the solid pass by
    /// triangles (CHANGELOG, "Кадр легче для процессора"), so "what got
    /// dearer" is first "whose triangles are these" -- and the F3 line only
    /// says how many. `FaceLayers::by_kind_for_test` makes every face's
    /// layer its block kind, so a mesh reads back as a census. Pieces of
    /// models drawn in another block's picture (a pole in the log's) count
    /// as that block: the table is of pictures, which is close enough to
    /// point at a culprit and is said here so nobody reads it as exact.
    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly, in release"]
    fn what_the_benchmark_scene_is_made_of() {
        use primitive_shared::worldgen::{Preset, Scale, Zone};
        // `saves/night/world.toml`: seed 4242, normal, temperate,
        // regional; spawn pinned at -16,24 by the benchmark
        // (`PRIMITIVE_TEST_SPAWN`).
        let generator = WorldGen::with_scale(4242, Preset::Normal, Zone::Temperate, Scale::Regional);
        let centre = ChunkPos::from_global(-16, 24).0;
        const FINE: i32 = 10;
        let around: Vec<ChunkPos> = (-(FINE + 1)..=FINE + 1)
            .flat_map(|dz| (-(FINE + 1)..=FINE + 1).map(move |dx| ChunkPos::new(centre.x + dx, centre.z + dz)))
            .collect();
        let generated: Vec<Chunk> = around.iter().map(|&pos| generator.generate_chunk(pos)).collect();
        let isolated: Vec<Vec<u8>> = generated.iter().map(|chunk| compute_isolated(&chunk.blocks)).collect();
        let mut chunks = ChunkManager::new(FINE + 2);
        for chunk in generated {
            chunks.insert(chunk);
        }
        let mut light = LightMap::new();
        for (pos, data) in around.iter().zip(isolated) {
            light.insert_precomputed(&chunks, *pos, data);
        }
        let layers = crate::engine::texture::FaceLayers::by_kind_for_test();
        let mut cache = Box::<Neighbourhood>::default();
        let mut out = Box::<MeshBuffers>::default();
        // [solid, the rest] triangles by layer.
        let mut census = vec![[0usize; 2]; crate::engine::mesh::MAX_TEXTURE_LAYERS as usize];
        let (mut meshing, mut meshed) = (0.0f64, 0usize);
        for pos in around.iter().filter(|p| (p.x - centre.x).abs() <= FINE && (p.z - centre.z).abs() <= FINE) {
            cache.fill(*pos, &chunks, &light);
            // As the game lays them out with the benchmark's settings
            // (`relief_chunks = 4`, `transparent_leaves_chunks = 6`), or
            // the census counts stones in relief nine chunks out that the
            // game lays flat past four.
            let distance = (((pos.x - centre.x).pow(2) + (pos.z - centre.z).pow(2)) as f32).sqrt();
            cache.lay_stones_flat(!crate::engine::lod::relief_at(distance, 4, true));
            cache.draw_leaves_solid(!crate::engine::lod::leaves_see_through_at(distance, 6, true));
            out.clear();
            let started = std::time::Instant::now();
            build_mesh(*pos, &cache, &layers, &generator, &mut out);
            meshing += started.elapsed().as_secs_f64();
            meshed += 1;
            for (i, tri) in out.indices.chunks_exact(3).enumerate() {
                let pass = usize::from(i * 3 >= out.solid_index_count as usize);
                census[out.vertices[tri[0] as usize].tex_layer() as usize][pass] += 1;
            }
        }
        // **The same ring meshed asking every model question per cell and
        // from `mesh::plain_cube`**, alternated and best of three each, so
        // the table's saving is measured on this ground rather than
        // assumed from a synthetic chunk.
        let inner: Vec<ChunkPos> = around.iter().copied().filter(|p| (p.x - centre.x).abs() <= FINE && (p.z - centre.z).abs() <= FINE).collect();
        let mut best = [f64::MAX; 2];
        for round in 0..6 {
            for flag in [round % 2 == 0, round % 2 != 0] {
                crate::engine::mesh::plain_cube::ASK_EVERYTHING.with(|ask| ask.set(!flag));
                let mut spent = 0.0;
                for pos in &inner {
                    cache.fill(*pos, &chunks, &light);
                    let distance = (((pos.x - centre.x).pow(2) + (pos.z - centre.z).pow(2)) as f32).sqrt();
                    cache.lay_stones_flat(!crate::engine::lod::relief_at(distance, 4, true));
                    cache.draw_leaves_solid(!crate::engine::lod::leaves_see_through_at(distance, 6, true));
                    let started = std::time::Instant::now();
                    build_mesh(*pos, &cache, &layers, &generator, &mut out);
                    spent += started.elapsed().as_secs_f64();
                }
                let slot = usize::from(flag);
                best[slot] = best[slot].min(spent * 1e3 / inner.len() as f64);
            }
        }
        crate::engine::mesh::plain_cube::ASK_EVERYTHING.with(|ask| ask.set(false));
        println!("[scene] build_mesh asking every question {:.3} ms a chunk, from the table {:.3}", best[0], best[1]);
        let total: [usize; 2] = census.iter().fold([0, 0], |a, c| [a[0] + c[0], a[1] + c[1]]);
        println!(
            "[scene] {meshed} chunks, {:.3} ms a chunk: {}k solid + {}k other triangles",
            meshing * 1e3 / meshed as f64,
            total[0] / 1000,
            total[1] / 1000
        );
        let mut rows: Vec<(usize, [usize; 2])> = census.into_iter().enumerate().filter(|(_, c)| c[0] + c[1] > 0).collect();
        rows.sort_by_key(|(_, c)| std::cmp::Reverse(c[0] + c[1]));
        for (layer, [solid, other]) in rows.into_iter().take(40) {
            let name = if (1..=1024).contains(&layer) {
                primitive_shared::types::block_name((layer - 1) as BlockId).to_string()
            } else {
                format!("extra/animal layer {layer}")
            };
            println!("[scene] {name:32} solid {:7} other {:7} ({:.1}%)", solid, other, (solid + other) as f64 * 100.0 / (total[0] + total[1]) as f64);
        }
    }

    /// What every coarse cell would cost on the same streamed world, split
    /// by LOD band and by how tall the chunk stands.
    ///
    /// ```text
    /// cargo test --release -p primitive_client --lib what_the_mountains_cost_band_by_band \
    ///     -- --ignored --nocapture
    /// ```
    ///
    /// `lod::CELL` was measured on a world sixty-four blocks tall, over
    /// meadow and shore, and it says 4x4 loses to 2x2. A world of 256 with
    /// ranges ninety blocks over the sea is a different question, and the
    /// answer can differ by terrain: so every chunk is meshed at every
    /// candidate cell and the table is cut by band and by height.
    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly, in release"]
    fn what_the_mountains_cost_band_by_band() {
        let generator = WorldGen::new(4242);
        let (sx, sz) = generator.spawn_column();
        measure_band_by_band(&generator, ChunkPos::from_global(sx, sz).0, "the benchmark's spawn");

        // The benchmark stands on a shore with hills and not one peak, so
        // it cannot say anything about a range: the same table again,
        // centred on the highest ground within a day's walk.
        let (mut peak, mut at) = (i32::MIN, (0, 0));
        for gz in (-1500..1500).step_by(24) {
            for gx in (-1500..1500).step_by(24) {
                let h = generator.height_at(gx, gz);
                if h > peak {
                    (peak, at) = (h, (gx, gz));
                }
            }
        }
        measure_band_by_band(
            &generator,
            ChunkPos::from_global(at.0, at.1).0,
            &format!("the highest ground, {peak} at {at:?}"),
        );
    }

    fn measure_band_by_band(generator: &WorldGen, centre: ChunkPos, label: &str) {
        use crate::engine::lod::{coarsen_cells, keeps_sprites, level_at, MAX_LEVEL};
        use primitive_shared::worldgen::SEA_LEVEL;

        let settings = crate::settings::ClientSettings::default();
        let probe = ChunkManager::new(RADIUS);
        let mut positions = Vec::new();
        for dx in -RADIUS..=RADIUS {
            for dz in -RADIUS..=RADIUS {
                if probe.inside(dx, dz) {
                    positions.push(ChunkPos::new(centre.x + dx, centre.z + dz));
                }
            }
        }
        let distance = |p: &ChunkPos| {
            let (dx, dz) = ((p.x - centre.x) as f32, (p.z - centre.z) as f32);
            (dx * dx + dz * dz).sqrt()
        };
        positions.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
        let generated: Vec<Chunk> = in_parallel(&positions, |&pos| generator.generate_chunk(pos));
        let isolated: Vec<Vec<u8>> = in_parallel(&generated, |chunk| compute_isolated(&chunk.blocks));
        let mut chunks = ChunkManager::new(RADIUS);
        for chunk in generated {
            chunks.insert(chunk);
        }
        let mut light = LightMap::new();
        for (pos, data) in positions.iter().zip(isolated) {
            light.insert_precomputed(&chunks, *pos, data);
        }

        let layers = crate::engine::texture::FaceLayers::numbered_for_test();
        let (lod, quality) = (settings.lod_distance_chunks, settings.lod_quality);
        const CELLS: [(usize, usize); 7] = [(1, 1), (2, 1), (4, 1), (2, 2), (4, 2), (8, 1), (8, 2)];
        // Band, skyline, then solid, every-pass and top-face triangles at
        // each of `CELLS`.
        type PerCell = [u64; 7];
        let rows: Vec<(u8, i32, PerCell, PerCell, PerCell)> = in_parallel(&positions, |pos| {
            let mut cache = Box::<Neighbourhood>::default();
            let mut out = Box::<MeshBuffers>::default();
            let band = level_at(distance(pos), lod, 0);
            let (mut solid, mut all, mut tops, mut ceiling) = ([0u64; 7], [0u64; 7], [0u64; 7], 0);
            for (i, cell) in CELLS.iter().enumerate() {
                cache.fill(*pos, &chunks, &light);
                ceiling = cache.ceiling();
                coarsen_cells(&mut cache, *cell, keeps_sprites(band.max(1), quality), quality.flat_light());
                out.clear();
                build_mesh(*pos, &cache, &layers, generator, &mut out);
                solid[i] = (out.solid_index_count / 3) as u64;
                // Face group 0 is +Y (see `mesh::faces()`): the tops, so
                // the rest of `solid` is walls and undersides.
                tops[i] = ((out.solid_groups[1] - out.solid_groups[0]) / 3) as u64;
                all[i] = (out.indices.len() / 3) as u64;
            }
            (band, ceiling, solid, all, tops)
        });

        println!();
        println!("{label}: {} chunks, lod from {lod} ({quality:?}); solid triangles in thousands", rows.len());
        println!("band  skyline          chunks   {:?}", CELLS);
        let classes = [
            ("low  < sea+32", i32::MIN, SEA_LEVEL + 32),
            ("hill < sea+64", SEA_LEVEL + 32, SEA_LEVEL + 64),
            ("peak >= sea+64", SEA_LEVEL + 64, i32::MAX),
        ];
        for band in 0..=MAX_LEVEL {
            for (name, lo, hi) in classes {
                let picked: Vec<_> = rows
                    .iter()
                    .filter(|r| r.0 == band && r.1 >= lo && r.1 < hi)
                    .collect();
                let sums: Vec<u64> = (0..CELLS.len())
                    .map(|i| picked.iter().map(|r| r.2[i]).sum::<u64>() / 1000)
                    .collect();
                let tops: Vec<u64> = (0..CELLS.len())
                    .map(|i| picked.iter().map(|r| r.4[i]).sum::<u64>() / 1000)
                    .collect();
                println!("{band:4}  {name:15}  {:6}   {sums:?}  tops {tops:?}", picked.len());
            }
        }
        // The same chunks under a mountain-aware start: the first band
        // starts nearer for a chunk whose skyline is tall. A coarse
        // chunk is costed at its stock cell, 2x2.
        let policy = |start_for: &dyn Fn(i32) -> i32| -> (u64, usize) {
            let (mut solid, mut fine) = (0u64, 0usize);
            for (pos, row) in positions.iter().zip(&rows) {
                let level = level_at(distance(pos), start_for(row.1), 0);
                solid += row.2[if level == 0 { 0 } else { 1 }];
                fine += usize::from(level == 0);
            }
            (solid / 1000, fine)
        };
        let (solid, fine) = policy(&|_| lod);
        println!("solid, the bands where the setting puts them: {solid}k ({fine} chunks at full detail)");
        for tall_from in [SEA_LEVEL + 24, SEA_LEVEL + 32, SEA_LEVEL + 42] {
            for share in [0.4f32, 0.5, 0.6, 0.7] {
                let (solid, fine) =
                    policy(&|skyline| crate::engine::lod::band_start_with(lod, skyline, tall_from, share));
                println!(
                    "  tall from sea+{}, first band at {share} of the setting: {solid}k ({fine} fine)",
                    tall_from - SEA_LEVEL
                );
            }
        }
        let (solid, fine) = policy(&|skyline| crate::engine::lod::band_start(lod, skyline));
        println!("  as shipped, `lod::band_start`: {solid}k ({fine} fine)");

        // What the stock table draws, all passes, for comparison with the
        // `tris=` total on the F3 line.
        let stock: u64 = rows
            .iter()
            .map(|r| r.3[if r.0 == 0 { 0 } else { 1 }])
            .sum();
        println!("stock policy, every pass: {}k triangles", stock / 1000);
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
    use super::{Arena, QUARANTINE_FRAMES};

    /// **What the F3 line says the arena costs is what it holds.** Used
    /// is what has been written and not yet given back -- including space
    /// still waiting out its quarantine, which the GPU may be reading --
    /// and allocated is the size of the two buffers, whatever is in them.
    #[test]
    fn usage_counts_what_was_written_and_what_the_buffers_are() {
        let Some((device, queue)) = gpu() else {
            println!("no GPU adapter on this machine; skipping the arena usage test");
            return;
        };
        let mut arena = Arena::with_capacity(device, 4, 64, 64);
        let allocated = 64 * 4 + 64 * 4;
        assert_eq!(arena.usage(), (0, allocated));

        let data: Vec<u32> = (0..40).collect();
        let (vertices, indices) = arena.upload(device, queue, &data, &data).expect("room");
        assert_eq!(arena.usage(), (40 * 4 + 40 * 4, allocated));

        arena.free(vertices, indices);
        assert_eq!(arena.usage().0, 40 * 4 + 40 * 4, "quarantined space was reported free");
        for _ in 0..QUARANTINE_FRAMES {
            arena.begin_frame();
        }
        assert_eq!(arena.usage(), (0, allocated));
    }


    /// The one shared device. See `engine::test_gpu` for why every test
    /// borrows the same one instead of making its own.
    fn gpu() -> Option<&'static (wgpu::Device, wgpu::Queue)> {
        crate::engine::test_gpu()
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
        let mut arena = Arena::with_capacity(device, 4, 64, 64);

        // Two uploads in a row with no submit between them -- which is
        // exactly what a frame of streaming terrain does, and exactly the
        // case that used to lose the first one.
        let first: Vec<u32> = (0..40).map(|i| 0xA000_0000 + i).collect();
        let second: Vec<u32> = (0..40).map(|i| 0xB000_0000 + i).collect();

        let before = arena.capacity();
        let (a_vertices, a_indices) =
            arena.upload(device, queue, &first, &first).expect("room for the first upload");
        let (b_vertices, b_indices) =
            arena.upload(device, queue, &second, &second).expect("room for the second upload");

        assert!(
            arena.capacity().0 > before.0 && arena.capacity().1 > before.1,
            "the second upload did not grow the arena, so this proves nothing"
        );
        assert_ne!(
            a_vertices.start, b_vertices.start,
            "two uploads landed in the same place"
        );

        let read_a = read_back(device, queue, &arena.vertex_buffer, a_vertices.start * 4, 40);
        assert_eq!(
            read_a, first,
            "the first upload was lost when the arena grew under it"
        );
        let read_b = read_back(device, queue, &arena.vertex_buffer, b_vertices.start * 4, 40);
        assert_eq!(read_b, second, "the upload that caused the growth was lost");

        // ...and the same for the index buffer, which grows independently.
        let read_a = read_back(device, queue, &arena.index_buffer, a_indices.start * 4, 40);
        assert_eq!(read_a, first, "the first upload's indices were lost");
        let read_b = read_back(device, queue, &arena.index_buffer, b_indices.start * 4, 40);
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
        let mut arena = Arena::with_capacity(device, 4, 32, 32);
        let mut placed = Vec::new();
        for chunk in 0..24u32 {
            let data: Vec<u32> = (0..16).map(|i| (chunk << 16) | i).collect();
            let (vertices, _) = arena
                .upload(device, queue, &data, &data)
                .expect("the arena is nowhere near this device's largest buffer");
            placed.push((vertices.start, data));
        }

        for (start, expected) in placed {
            let got = read_back(device, queue, &arena.vertex_buffer, start * 4, expected.len());
            assert_eq!(got, expected, "an upload at {start} did not survive");
        }
    }
}
