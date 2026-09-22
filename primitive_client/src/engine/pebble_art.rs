//! The pictures of the rocks' pebbles, made at load rather than drawn.
//!
//! **Fourteen pebbles that differ in nothing but their rock.** A pebble's
//! shape is the pebble's -- a few round stones lying together, the picture in
//! `terrain/pebble.png` -- and what makes a pebble of granite granite is the
//! granite. So a rock's pebble is the pebble's shape and light, filled with
//! the rock's own cobble (`rocks/<rock>_cobble.png`): every texel of the
//! pebble that is a stone takes the cobble's colour at that place, darkened or
//! lightened by how bright the pebble picture is there against its own mean,
//! which is what keeps the round tops lit and the undersides in shadow.
//!
//! Rejected: **a picture per rock** -- fourteen files that had to be redrawn
//! whenever the pebble's shape or a rock's cobble changed, and that drifted
//! from both ("удали текстуры камушков, их пусть код делает"); **one pebble
//! tinted per rock** -- a tint is one colour, and a speckled granite or a
//! banded gneiss is not one colour.
//!
//! A file on disk under the same name still wins: `generate` is asked only
//! for names the loader could not find, so an artist who wants one rock's
//! pebble drawn by hand draws it.

use image::{Rgba, RgbaImage};

/// Whether this texture name is one this module makes. Asked by the tests
/// that hold every named picture to being in the binary.
#[cfg(test)]
pub fn is_generated(name: &str) -> bool {
    rock_of(name).is_some()
}

/// `rocks/granite_pebble.png` -> `granite`.
fn rock_of(name: &str) -> Option<&str> {
    name.strip_prefix("rocks/")?.strip_suffix("_pebble.png")
}

/// The pebble of the rock `name` names, at the atlas resolution, or `None`
/// for a name this does not make or a rock with no cobble to fill it from.
pub fn generate(name: &str, resolution: u32) -> Option<RgbaImage> {
    let rock = rock_of(name)?;
    let shape = picture("terrain/pebble.png", resolution)?;
    let cobble = picture(&format!("rocks/{rock}_cobble.png"), resolution)?;
    Some(fill(&shape, &cobble))
}

fn picture(name: &str, resolution: u32) -> Option<RgbaImage> {
    let bytes = crate::embedded::texture(name)?;
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(image::imageops::resize(&img, resolution, resolution, image::imageops::FilterType::Nearest))
}

fn luminance(p: &Rgba<u8>) -> f32 {
    0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2])
}

/// The shape's stones, in the cobble's colours, with the shape's light.
fn fill(shape: &RgbaImage, cobble: &RgbaImage) -> RgbaImage {
    let lit: Vec<f32> = shape.pixels().filter(|p| p[3] > 0).map(luminance).collect();
    let mean = (lit.iter().sum::<f32>() / lit.len().max(1) as f32).max(1.0);
    let mut out = RgbaImage::new(shape.width(), shape.height());
    for (x, y, p) in shape.enumerate_pixels() {
        if p[3] == 0 {
            continue;
        }
        let light = (luminance(p) / mean).clamp(0.45, 1.6);
        let c = cobble.get_pixel(x % cobble.width(), y % cobble.height());
        let tone = |v: u8| (f32::from(v) * light).round().clamp(0.0, 255.0) as u8;
        out.put_pixel(x, y, Rgba([tone(c[0]), tone(c[1]), tone(c[2]), p[3]]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rocks_pebble_is_made_from_its_own_cobble_in_the_pebbles_shape() {
        let names: Vec<String> = crate::embedded::BLOCKS_TOML
            .split('"')
            .filter(|piece| is_generated(piece))
            .map(str::to_string)
            .collect();
        assert!(names.len() >= 10, "only {} rock pebbles are asked for", names.len());
        let shape = picture("terrain/pebble.png", 16).expect("the pebble's shape");
        let mut seen = Vec::new();
        for name in &names {
            let made = generate(name, 16).unwrap_or_else(|| panic!("{name} could not be made"));
            for (a, b) in made.pixels().zip(shape.pixels()) {
                assert_eq!(a[3] > 0, b[3] > 0, "{name} is not the pebble's shape");
            }
            seen.push(made.into_raw());
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), names.len(), "two rocks made the same pebble");
    }
}
