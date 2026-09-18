//! One frame of the world, written to a PNG.
//!
//! ## Why this exists
//!
//! `ui::snapshot` makes the same argument about the interface and makes
//! it well: *an interface that can only be reviewed by launching the
//! game and taking a photograph is an interface that gets reviewed
//! once.* All of that is true of the world too, and more so -- the
//! interface is a pure function of its inputs and can be drawn without a
//! graphics card at all, while the world is the graphics card.
//!
//! So this is the other half. It cannot skip the GPU, but it can skip
//! the human: point the game at a world, tell it which frame to keep,
//! and get a file.
//!
//! ```text
//! PRIMITIVE_AUTOSTART=myworld PRIMITIVE_SHOT=shot.png \
//!     PRIMITIVE_SHOT_AFTER=6 cargo run --release -p primitive_client
//! ```
//!
//! It waits `PRIMITIVE_SHOT_AFTER` seconds (default six) so the terrain
//! has arrived and the light has settled, writes the file, and -- with
//! `PRIMITIVE_BENCH` alongside it -- the run ends on its own. What comes
//! out is what the player sees, at whatever the window's size is,
//! interface included.
//!
//! ## Why it is a whole file for one copy
//!
//! Because the copy is the easy part and every line around it is a rule
//! about how GPUs hand memory back:
//!
//! * a buffer's rows are padded to 256 bytes, and a screenshot that
//!   ignores that is sheared diagonally;
//! * the surface is very often `Bgra`, and one that ignores *that* comes
//!   out with the sky orange, which is exactly the kind of bug that gets
//!   mistaken for the thing being tested;
//! * mapping is asynchronous, and reading before the device has been
//!   polled gets nothing.
//!
//! Each of those is one line and a paragraph, and none of them belongs
//! in the middle of `render`.

use std::path::{Path, PathBuf};

/// A screenshot that has been asked for but not taken yet.
///
/// Read from the environment once at startup rather than checked per
/// frame: it is a development tool, and a `getenv` on the frame path to
/// serve a feature nobody has switched on is exactly the sort of thing
/// this codebase argues against elsewhere.
pub struct Pending {
    pub path: PathBuf,
    /// Seconds to wait before taking it.
    pub after: f32,
}

impl Pending {
    /// What the environment asked for, if anything.
    pub fn from_env() -> Option<Pending> {
        let path = std::env::var("PRIMITIVE_SHOT").ok()?;
        if path.trim().is_empty() {
            return None;
        }
        let after = std::env::var("PRIMITIVE_SHOT_AFTER")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|s| s.is_finite() && *s >= 0.0)
            // Six seconds: long enough for the chunks around the spawn
            // to arrive and be meshed on a cold cache, which is what
            // separates a screenshot of the world from a screenshot of
            // the fog.
            .unwrap_or(6.0);
        Some(Pending { path: PathBuf::from(path), after })
    }
}

/// A view direction forced from the environment, and the sweep it turns
/// through.
///
/// **Why the game needs this at all.** A whole class of rendering fault
/// -- the one this was written for -- is invisible in a still frame and
/// obvious the moment the player turns: single pixels that flicker as
/// the sub-pixel alignment between the geometry and the pixel grid
/// changes. `PRIMITIVE_SHOT` photographs the view the save file happens
/// to hold, once, and a still photograph of a flicker is a photograph of
/// nothing. Turning the camera to look at the fault, and turning it a
/// fraction of a degree between two frames, are both things that were
/// only possible with a hand on a mouse -- and this project's rule is
/// that when something is reachable only by hand, the fix is the missing
/// hook rather than a hand-run experiment nobody can repeat.
///
/// ```text
/// PRIMITIVE_LOOK="135 -8"        # yaw and pitch in degrees
/// PRIMITIVE_LOOK_SWEEP="16x0.2"  # sixteen shots, a fifth of a degree apart
/// ```
///
/// Yaw is measured the way `Camera::forward` builds it: 0 looks along
/// +X, 90 along +Z. Pitch is positive upward. Degrees rather than
/// radians because the number is typed by a person on a command line.
///
/// With a sweep, `PRIMITIVE_SHOT=shot.png` writes `shot.000.png`,
/// `shot.001.png` and so on, one per frame, so the files can be
/// subtracted from each other. **Flicker is the difference between
/// frames, not the brightness inside one** -- a detector that looks for
/// bright pixels in a single frame finds the texture's own grain and
/// says it found the bug, which is how the previous attempt at this
/// went wrong.
pub struct Look {
    pub yaw_degrees: f32,
    pub pitch_degrees: f32,
    /// Degrees of yaw between one shot and the next.
    pub sweep_step: f32,
    /// How many shots the sweep is. One when no sweep was asked for.
    pub frames: u32,
}

impl Look {
    pub fn from_env() -> Option<Look> {
        let raw = std::env::var("PRIMITIVE_LOOK").ok()?;
        let mut parts = raw.split_whitespace();
        let yaw_degrees: f32 = parts.next()?.parse().ok()?;
        // Pitch is optional: level is the common case and typing a
        // second zero for it is noise.
        let pitch_degrees: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        if !yaw_degrees.is_finite() || !pitch_degrees.is_finite() {
            return None;
        }
        let (frames, sweep_step) = parse_sweep(std::env::var("PRIMITIVE_LOOK_SWEEP").ok());
        Some(Look {
            yaw_degrees,
            pitch_degrees,
            sweep_step,
            frames,
        })
    }

    /// Where the camera looks for shot `index`, in radians.
    pub fn angles(&self, index: u32) -> (f32, f32) {
        let yaw = self.yaw_degrees + self.sweep_step * index as f32;
        (yaw.to_radians(), self.pitch_degrees.to_radians())
    }
}

/// `"<count>x<step>"`, e.g. `16x0.2`.
///
/// Split out so it can be tested without touching the process
/// environment -- `from_env` cannot be, and a parser that is only
/// exercised through a variable is a parser nobody checks.
fn parse_sweep(raw: Option<String>) -> (u32, f32) {
    let Some(raw) = raw else {
        return (1, 0.0);
    };
    let (count, step) = match raw.split_once(['x', 'X']) {
        Some(pair) => pair,
        None => return (1, 0.0),
    };
    let count: u32 = count.trim().parse().unwrap_or(0);
    let step: f32 = step.trim().parse().unwrap_or(f32::NAN);
    if count < 1 || !step.is_finite() {
        return (1, 0.0);
    }
    // A cap, because the frames land one per frame and a typo with an
    // extra zero would fill a directory before anybody could stop it.
    (count.min(512), step)
}

/// The file for one shot of a sweep: `shot.png` becomes `shot.007.png`.
///
/// Numbered rather than suffixed with the angle, because the angles are
/// fractions of a degree and a file called `shot.-0.20.png` sorts
/// nowhere useful. A single shot keeps the name it was given, so the
/// plain `PRIMITIVE_SHOT` case is unchanged.
pub fn frame_path(base: &Path, index: u32, frames: u32) -> PathBuf {
    if frames <= 1 {
        return base.to_path_buf();
    }
    let stem = base.file_stem().map(|s| s.to_string_lossy().into_owned());
    let extension = base
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "png".to_string());
    let stem = stem.unwrap_or_else(|| "shot".to_string());
    base.with_file_name(format!("{stem}.{index:03}.{extension}"))
}

/// Copies a rendered texture back and writes it out as a PNG.
///
/// Takes the texture rather than the surface: the caller has the frame
/// in hand and this has no business knowing where it came from.
///
/// Errors are returned rather than logged, because the caller is the one
/// that knows whether a failed screenshot should end the run.
pub fn write_png(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("the window has no pixels".to_string());
    }

    // **Rows are padded to 256 bytes.** A buffer copy has to be laid out
    // the way the hardware wants it, not the way the image does, so the
    // stride here is nearly always wider than the picture and the rows
    // are trimmed on the way out.
    const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let unpadded = width * 4;
    let padded = unpadded.div_ceil(ALIGN) * ALIGN;

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot readback"),
        size: (padded as u64) * (height as u64),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("screenshot"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    // Mapping is asynchronous and the callback only runs while the
    // device is being polled. `Maintain::Wait` blocks until the queue
    // has drained, which is the whole of the synchronisation this needs
    // -- a screenshot is allowed to stall the frame it is taken on.
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv()
        .map_err(|_| "the graphics device went away".to_string())?
        .map_err(|e| format!("could not read the frame back: {e}"))?;

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((unpadded as usize) * (height as usize));
    for row in 0..height as usize {
        let start = row * padded as usize;
        pixels.extend_from_slice(&mapped[start..start + unpadded as usize]);
    }
    drop(mapped);
    buffer.unmap();

    // **Which way round the channels are.** Surfaces are very often
    // `Bgra`, and getting this wrong produces a perfectly sharp
    // screenshot with an orange sky -- which looks like a rendering bug
    // rather than like a screenshot bug, and would be blamed on
    // whatever was being photographed.
    if matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }
    // The surface has an alpha channel and nothing has written anything
    // meaningful to it. Left as it came, a screenshot opens as a
    // transparent rectangle in half the world's image viewers.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not make {}: {e}", parent.display()))?;
        }
    }
    image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| "the frame came back the wrong size".to_string())?
        .save(path)
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The padding rule, on its own, because it is the one that is
    /// silent when it is wrong: an unpadded stride does not fail, it
    /// shears the picture diagonally and looks like a driver bug.
    #[test]
    fn rows_are_padded_up_to_the_alignment() {
        const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        for width in [1u32, 63, 64, 65, 320, 1280, 1920, 2560] {
            let unpadded = width * 4;
            let padded = unpadded.div_ceil(ALIGN) * ALIGN;
            assert!(padded >= unpadded, "width {width} lost bytes");
            assert_eq!(padded % ALIGN, 0, "width {width} is not aligned");
            assert!(
                padded - unpadded < ALIGN,
                "width {width} was padded by a whole row"
            );
        }
    }

    /// The sweep is what makes two frames a fifth of a degree apart
    /// possible, and a sweep that silently parses as "one frame" turns
    /// the experiment back into a single still without saying so.
    #[test]
    fn a_sweep_is_a_count_and_a_step_or_it_is_a_single_frame() {
        assert_eq!(parse_sweep(Some("16x0.2".into())), (16, 0.2));
        assert_eq!(parse_sweep(Some(" 4 X -1.5 ".into())), (4, -1.5));
        // Nothing asked for, nonsense asked for, and impossible asked
        // for all mean the same thing: the one frame `PRIMITIVE_SHOT`
        // already promised.
        assert_eq!(parse_sweep(None), (1, 0.0));
        assert_eq!(parse_sweep(Some("16".into())), (1, 0.0));
        assert_eq!(parse_sweep(Some("0x1".into())), (1, 0.0));
        assert_eq!(parse_sweep(Some("axb".into())), (1, 0.0));
        // ...and a typed extra zero does not fill a directory.
        assert_eq!(parse_sweep(Some("100000x0.1".into())).0, 512);
    }

    /// Numbered files, and the single-shot case left exactly as it was.
    #[test]
    fn a_swept_shot_is_numbered_and_a_lone_shot_is_not() {
        let base = Path::new("shots/beach.png");
        assert_eq!(frame_path(base, 0, 1), PathBuf::from("shots/beach.png"));
        assert_eq!(
            frame_path(base, 7, 16),
            PathBuf::from("shots/beach.007.png")
        );
    }

    #[test]
    fn nothing_is_asked_for_unless_it_is_asked_for() {
        // `from_env` reads the process environment, so it cannot be
        // exercised both ways without a lock that every other test would
        // have to know about. What is worth checking is the shape of the
        // answer for the case that costs something: an empty variable is
        // somebody clearing it, not somebody asking for a file called
        // "".
        std::env::set_var("PRIMITIVE_SHOT", "   ");
        assert!(Pending::from_env().is_none());
        std::env::remove_var("PRIMITIVE_SHOT");
        assert!(Pending::from_env().is_none());
    }
}
