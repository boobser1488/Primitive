//! How long the GPU actually spends on a frame.
//!
//! **The one number the frame timer cannot see.** `simulation`,
//! `encode` and `present` are all measured with a clock on the CPU, and
//! between them they account for a frame the way a receipt accounts for
//! an afternoon: the totals are right and none of it says where the time
//! went. On a client that submits work and then waits, almost the whole
//! frame lands in "waiting", and waiting is not a thing that can be
//! optimised -- it is the shape of whatever the GPU is doing, seen from
//! the wrong side.
//!
//! So this asks the GPU. Two timestamps, written by the GPU itself at
//! the start and end of the render pass, differenced and scaled by the
//! period the queue reports. What comes back is the time the hardware
//! spent on the pass and nothing else: no swapchain wait, no driver
//! bookkeeping, no scheduler.
//!
//! The difference between that and the wall-clock frame is the whole
//! point of having it. A frame of 1.3 ms whose pass is 1.2 ms is a
//! frame that would go faster with less geometry. A frame of 1.3 ms
//! whose pass is 0.2 ms is a frame spending a millisecond somewhere no
//! amount of meshing will reach, and drawing less would buy nothing --
//! which is a conclusion worth being able to *reach* rather than
//! assume.
//!
//! ## Reading it back without stalling
//!
//! A timestamp is written by the GPU, so it cannot be read until the GPU
//! has got there, and waiting for that would cost far more than it
//! measures. Instead the resolved values go into a small ring of
//! mappable buffers: each frame claims a free slot, and slots are read
//! whenever they happen to be ready, which is a frame or three later.
//! The number in the overlay is therefore slightly stale and exactly
//! right, which is the correct trade for something that is read by eye
//! once a second.
//!
//! ## When it is not there
//!
//! `TIMESTAMP_QUERY` is optional, and a driver that does not offer it
//! must not cost anyone a client that refuses to start. Everything here
//! is behind an `Option`: unsupported means the overlay shows a dash
//! where the number would be, and nothing else changes.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

/// How many frames of readback are in flight.
///
/// Three: enough that a frame nearly always finds a free slot even when
/// the GPU is two frames behind, few enough that the numbers are never
/// old enough to mislead.
const SLOTS: usize = 3;

/// The sections of the frame, in the order the pass draws them.
///
/// The whole-pass number said the frame was fill-bound and said nothing
/// about *which* fill. These do: each is a stage of the one render pass,
/// and the marks between them are written by the GPU as it reaches them.
pub const STAGES: [&str; 6] = ["solid", "cutout", "sky", "actors", "water", "ui"];

/// Timestamps per frame: the start of the pass, one at the end of every
/// stage, the end of the pass, and two more for the small sky.
///
/// The sky at reduced scale is drawn in a pass of its own *before* the
/// frame -- its own target, no depth -- so the marks inside the main
/// pass cannot see it. Left unmeasured it made the sky look free: the
/// `sky` stage fell to a twentieth of a millisecond and the work had
/// simply moved somewhere nothing was watching.
const OFFSCREEN_BEGIN: u32 = 2 + STAGES.len() as u32;
const OFFSCREEN_END: u32 = OFFSCREEN_BEGIN + 1;
const TIMESTAMPS: u32 = OFFSCREEN_END + 1;

/// Bytes the resolve writes. `resolve_query_set` emits one `u64` each.
const RESOLVED_BYTES: u64 = TIMESTAMPS as u64 * 8;

/// `resolve_query_set` requires its destination offset to be aligned,
/// and the simplest way to never think about it again is to give every
/// buffer a whole alignment unit to itself.
const SLOT_BYTES: u64 = wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;

// Slot states. A plain atomic rather than a channel: the map callback
// has to be `'static` and this is the whole of what it needs to say.
const FREE: u8 = 0;
const IN_FLIGHT: u8 = 1;
const READY: u8 = 2;

struct Slot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    /// Whether the frame in this slot drew the sky in a pass of its own.
    ///
    /// Kept per slot rather than on the timer because the answer is
    /// read back a frame or three after it was true, by which time the
    /// setting may have changed.
    offscreen: bool,
}

pub struct GpuTiming {
    query_set: wgpu::QuerySet,
    /// Where `resolve_query_set` puts the raw ticks before they are
    /// copied somewhere the CPU is allowed to look.
    resolve: wgpu::Buffer,
    slots: [Slot; SLOTS],
    /// Which slot this frame is using, if it found a free one.
    claimed: Option<usize>,
    /// Nanoseconds per tick, from the queue.
    period_ns: f32,
    /// The most recent completed measurement of the whole pass.
    last_ms: Option<f32>,
    /// ...and of each stage, in `STAGES` order. Empty where the driver
    /// will not write timestamps inside a pass.
    last_stage_ms: [f32; STAGES.len()],
    /// Whether marks may be written mid-pass at all.
    stages_supported: bool,
}

impl GpuTiming {
    /// The features to ask the device for, of those the adapter has.
    ///
    /// Both, and neither is required. `TIMESTAMP_QUERY` alone gives the
    /// pass as a whole; `TIMESTAMP_QUERY_INSIDE_PASSES` is the less
    /// widely supported one that allows marks between the stages, and
    /// asking for it as a hard requirement would lose the measurement
    /// entirely on the hardware most likely to need it.
    pub fn wanted(adapter: &wgpu::Adapter) -> wgpu::Features {
        adapter.features()
            & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES)
    }

    /// `None` when the device was created without the feature, which is
    /// not an error and not worth a warning louder than the dash in the
    /// overlay.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("frame timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: TIMESTAMPS,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp resolve"),
            size: SLOT_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = std::array::from_fn(|_| Slot {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("timestamp readback"),
                size: SLOT_BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(FREE)),
            offscreen: false,
        });
        Some(Self {
            query_set,
            resolve,
            slots,
            claimed: None,
            period_ns: queue.get_timestamp_period(),
            last_ms: None,
            last_stage_ms: [0.0; STAGES.len()],
            stages_supported: device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
        })
    }

    /// What to hand the render pass so the GPU stamps its own start and
    /// end. Claims a readback slot for this frame; if none is free --
    /// which means the GPU is more than `SLOTS` frames behind -- the
    /// frame simply goes unmeasured rather than waiting for one.
    pub fn pass_writes(
        &mut self,
    ) -> Option<(
        wgpu::RenderPassTimestampWrites<'_>,
        Option<&wgpu::QuerySet>,
        wgpu::RenderPassTimestampWrites<'_>,
    )> {
        self.claimed = self
            .slots
            .iter()
            .position(|slot| slot.state.load(Ordering::Acquire) == FREE);
        self.claimed?;
        let writes = wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        };
        // The same query set handed back for the mid-pass marks, or
        // nothing where the driver refuses to write them. Both borrows
        // are shared and come from the same reborrow, so the pass can
        // hold one while `mark` uses the other.
        let marks = self.stages_supported.then_some(&self.query_set);
        // The small sky's own pass, handed out here rather than asked
        // for separately: everything the frame needs comes from one
        // borrow, so the two passes can hold their marks at once.
        let offscreen = wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(OFFSCREEN_BEGIN),
            end_of_pass_write_index: Some(OFFSCREEN_END),
        };
        Some((writes, marks, offscreen))
    }

    /// The query index a stage's closing mark is written to.
    ///
    /// Zero and one are the pass boundaries, so the stages start at two.
    pub fn stage_query(stage: usize) -> u32 {
        stage as u32 + 2
    }

    /// Queues the copy out of the query set. Must be called on the same
    /// encoder as the pass, after it has ended.
    ///
    /// **`offscreen` is not optional information.** A query that was
    /// never written must not be resolved -- the result is undefined,
    /// and on this driver it is a hang rather than a wrong number. When
    /// the sky is drawn at full size there is no second pass, nothing
    /// writes the last two timestamps, and the resolve has to stop
    /// short of them.
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder, offscreen: bool) {
        let Some(slot) = self.claimed else {
            return;
        };
        self.slots[slot].offscreen = offscreen;
        let count = if offscreen { TIMESTAMPS } else { OFFSCREEN_BEGIN };
        encoder.resolve_query_set(&self.query_set, 0..count, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolve,
            0,
            &self.slots[slot].buffer,
            0,
            RESOLVED_BYTES,
        );
    }

    /// Call after `submit`. Starts the mapping for this frame's slot and
    /// harvests whatever earlier slot has come back.
    ///
    /// The poll is what runs the map callbacks. `Poll` rather than
    /// `Wait`: this must never block the frame on the GPU, and a slot
    /// that is not ready yet will be ready in a frame or two.
    pub fn after_submit(&mut self, device: &wgpu::Device) {
        if let Some(index) = self.claimed.take() {
            let slot = &self.slots[index];
            slot.state.store(IN_FLIGHT, Ordering::Release);
            let state = Arc::clone(&slot.state);
            slot.buffer
                .slice(..RESOLVED_BYTES)
                .map_async(wgpu::MapMode::Read, move |result| {
                    // A failed map is not worth a message: the slot goes
                    // back into circulation and the next frame measures
                    // instead.
                    state.store(if result.is_ok() { READY } else { FREE }, Ordering::Release);
                });
        }
        device.poll(wgpu::Maintain::Poll);
        self.harvest();
    }

    /// Reads any slot the driver has finished with.
    fn harvest(&mut self) {
        for slot in &self.slots {
            if slot.state.load(Ordering::Acquire) != READY {
                continue;
            }
            {
                let view = slot.buffer.slice(..RESOLVED_BYTES).get_mapped_range();
                let ticks: &[u64] = bytemuck::cast_slice(&view);
                // Timestamps are not promised to be monotonic across a
                // reset, and a wrapped pair would otherwise show up as
                // an enormous frame.
                let to_ms = |from: u64, to: u64| {
                    to.checked_sub(from)
                        .map(|ticks| ticks as f32 * self.period_ns / 1e6)
                };
                // The small sky's pass, when there was one. Its marks
                // are left at whatever the previous frame wrote when
                // the sky is drawn at full scale, so a zero-or-negative
                // difference is read as "did not happen".
                let offscreen = if slot.offscreen {
                    to_ms(ticks[OFFSCREEN_BEGIN as usize], ticks[OFFSCREEN_END as usize])
                        .filter(|ms| *ms > 0.0)
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                if let Some(whole) = to_ms(ticks[0], ticks[1]) {
                    // Everything the GPU did this frame, both passes.
                    self.last_ms = Some(whole + offscreen);
                }
                if self.stages_supported {
                    // Each stage runs from wherever the previous one
                    // finished, and the first from the start of the pass.
                    let mut previous = ticks[0];
                    for stage in 0..STAGES.len() {
                        let at = ticks[Self::stage_query(stage) as usize];
                        self.last_stage_ms[stage] = to_ms(previous, at).unwrap_or(0.0);
                        previous = at;
                    }
                    // The small sky belongs to the sky, wherever it was
                    // drawn. Index 2 is `sky` in `STAGES`.
                    self.last_stage_ms[2] += offscreen;
                }
            }
            slot.buffer.unmap();
            slot.state.store(FREE, Ordering::Release);
        }
    }

    /// GPU milliseconds for the render pass, from the most recent frame
    /// whose timestamps have made it back.
    pub fn last_ms(&self) -> Option<f32> {
        self.last_ms
    }

    /// The same, split by stage in `STAGES` order. `None` where the
    /// driver will not write timestamps inside a pass.
    pub fn last_stage_ms(&self) -> Option<&[f32; STAGES.len()]> {
        self.stages_supported.then_some(&self.last_stage_ms)
    }
}
