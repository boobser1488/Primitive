//! Compiles every WGSL shader with naga -- the same front end wgpu uses.
//!
//! Why this exists: a shader typo, or a mismatch between the Rust vertex
//! struct and the shader's `@location` list, doesn't fail the build. It
//! fails at `create_render_pipeline` -- i.e. only on a machine with a
//! working GPU, only at runtime, and usually as a wall of validation
//! text. This turns all of that into an ordinary test failure.

use naga::valid::{Capabilities, ValidationFlags, Validator};

fn validate(name: &str, source: &str) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{name} failed to parse:\n{}", e.emit_to_string(source)));

    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name} failed validation: {e:?}"));
}

#[test]
fn chunk_shader_compiles() {
    validate("shader.wgsl", include_str!("../src/shader.wgsl"));
}

#[test]
fn actor_shader_compiles() {
    validate("actor.wgsl", include_str!("../src/actor.wgsl"));
}

#[test]
fn overlay_shader_compiles() {
    validate("overlay.wgsl", include_str!("../src/overlay.wgsl"));
}

/// The chunk shader's vertex inputs must line up with `mesh::Vertex`'s
/// attribute list, and the actor shader's with `ActorVertex`. Checking
/// the declared locations catches the classic "added a field to the Rust
/// struct, forgot the shader" desync.
#[test]
fn vertex_inputs_match_the_rust_structs() {
    let chunk = include_str!("../src/shader.wgsl");
    for location in ["@location(0)", "@location(1)", "@location(2)", "@location(3)"] {
        assert!(
            chunk.contains(location),
            "shader.wgsl is missing {location}; mesh::Vertex declares 4 attributes"
        );
    }

    let actor = include_str!("../src/actor.wgsl");
    for location in ["@location(0)", "@location(1)", "@location(2)"] {
        assert!(
            actor.contains(location),
            "actor.wgsl is missing {location}; ActorVertex declares 3 attributes \
             (position, color, normal)"
        );
    }
}

/// The entry points the pipelines ask for by name must exist.
///
/// Naga validating the module does not catch a renamed entry point:
/// the shader is still valid, and the failure surfaces only when
/// `create_render_pipeline` runs on a machine with a GPU -- which is to
/// say, on a player's machine and not in CI.
#[test]
fn the_chunk_shader_exposes_the_entry_points_the_pipelines_ask_for() {
    let source = include_str!("../src/shader.wgsl");
    let module = naga::front::wgsl::parse_str(source).expect("chunk shader should parse");

    let names: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
    for wanted in ["vs_main", "fs_solid", "fs_cutout"] {
        assert!(
            names.contains(&wanted),
            "shader.wgsl has no entry point {wanted:?}; it has {names:?}"
        );
    }
}

/// Only the cutout entry point may discard.
///
/// A fragment shader that can discard forces the GPU to run it before it
/// knows whether the fragment survives, so early depth rejection is off
/// for every draw using it. The solid pass carries most of the frame's
/// triangles and must not pay that -- which is the whole reason the
/// shader has two entry points rather than one.
#[test]
fn the_solid_entry_point_does_not_discard() {
    let source = include_str!("../src/shader.wgsl");
    let solid = source
        .split("fn fs_solid")
        .nth(1)
        .and_then(|rest| rest.split("\n}").next())
        .expect("fs_solid should be in the shader");
    assert!(
        !solid.contains("discard"),
        "fs_solid discards, which costs the terrain its early-Z:\n{solid}"
    );
    assert!(
        source.contains("fn fs_cutout"),
        "the discarding entry point should still exist"
    );
}
