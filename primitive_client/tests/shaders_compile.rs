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
    validate("shader.wgsl", include_str!("../src/engine/shader.wgsl"));
}

/// ...and at every lighting step, not only the one the file is written at.
///
/// Every step past Simple is the same text with one constant rewritten, and
/// the branches that constant folds away are code naga still has to accept.
/// A model entry point that only compiles at Simple is one that fails on the
/// frame a player presses the row.
#[test]
fn chunk_shader_compiles_at_every_lighting_step() {
    let source = include_str!("../src/engine/shader.wgsl");
    const SWITCH: &str = "const LIGHTING: u32 = 0u;";
    assert!(source.contains(SWITCH), "shader.wgsl lost its lighting switch");
    for step in 1..=2 {
        validate(
            &format!("shader.wgsl at step {step}"),
            &source.replacen(SWITCH, &format!("const LIGHTING: u32 = {step}u;"), 1),
        );
    }
}

#[test]
fn sky_shader_compiles() {
    validate("sky.wgsl", include_str!("../src/engine/sky.wgsl"));
}

#[test]
fn sky_blit_shader_compiles() {
    validate("sky_blit.wgsl", include_str!("../src/engine/sky_blit.wgsl"));
}

/// The sky pass has no vertex buffer: its triangle is three points
/// computed from the vertex index. A `@location` in its vertex input
/// would mean it had grown one without anyone saying so.
#[test]
fn the_sky_needs_no_vertex_buffer() {
    let source = include_str!("../src/engine/sky.wgsl");
    let module = naga::front::wgsl::parse_str(source).expect("sky shader should parse");
    let vertex = module
        .entry_points
        .iter()
        .find(|e| e.name == "vs_sky")
        .expect("no vs_sky");
    for argument in &vertex.function.arguments {
        assert!(
            !matches!(argument.binding, Some(naga::Binding::Location { .. })),
            "vs_sky takes a vertex attribute, so the pipeline needs a buffer it does not have"
        );
    }
}

/// The hand is the one thing drawn with a matrix no other entry point uses.
///
/// `hand_view_proj` sits well down the shared `Globals` block, and a
/// declaration of the fields before it in the wrong order would read some
/// other frame parameter as a matrix -- which is not a validation error, it
/// is a hand somewhere off the side of the world. It was its own file; it is
/// `vs_held` in the chunk shader now, which declares the whole block.
#[test]
fn the_hand_reads_its_own_projection_where_the_renderer_writes_it() {
    let source = include_str!("../src/engine/shader.wgsl");
    let fields = [
        "view_proj",
        "camera_pos",
        "sun",
        "fog_color",
        "fog_params",
        "extra",
        "texture_params",
        "inv_view_proj",
        "sky_params",
        "render_origin",
        "hand_view_proj",
    ];
    let mut at = source.find("struct Globals {").expect("shader.wgsl declares Globals");
    for field in fields {
        let found = source[at..]
            .find(&format!("{field}:"))
            .unwrap_or_else(|| panic!("shader.wgsl declares no {field}, or declares it out of order"));
        at += found + field.len();
    }
    assert!(source.contains("globals.hand_view_proj"), "nothing reads the hand's projection");
}

#[test]
fn overlay_shader_compiles() {
    validate("overlay.wgsl", include_str!("../src/engine/overlay.wgsl"));
}

/// The body of a `struct` in WGSL source, from its opening brace to its
/// closing one.
fn struct_body<'a>(source: &'a str, name: &str) -> &'a str {
    let head = format!("struct {name} {{");
    let start = source.find(&head).unwrap_or_else(|| panic!("no struct {name}")) + head.len();
    &source[start..start + source[start..].find('}').expect("an unterminated struct would not compile")]
}

/// The chunk shader's vertex inputs must line up with the Rust structs that
/// fill them: `mesh::Vertex`, and for the models `ActorVertex` and
/// `HandVertex`. Checking the declared locations catches the classic "added a
/// field to the Rust struct, forgot the shader" desync -- which this file
/// caught for real when a figure's light was added to `ActorVertex`.
#[test]
fn vertex_inputs_match_the_rust_structs() {
    let chunk = include_str!("../src/engine/shader.wgsl");
    for location in ["@location(0)", "@location(1)", "@location(2)", "@location(3)"] {
        assert!(
            struct_body(chunk, "VertexInput").contains(location),
            "shader.wgsl's VertexInput is missing {location}; mesh::Vertex declares 4 attributes"
        );
    }

    let actor = struct_body(chunk, "ActorVertexInput");
    for location in ["@location(0)", "@location(1)", "@location(2)", "@location(3)", "@location(4)"] {
        assert!(
            actor.contains(location),
            "ActorVertexInput is missing {location}; ActorVertex declares 5 attributes \
             (position, color, normal, uv, light)"
        );
    }

    let hand = struct_body(chunk, "HeldVertexInput");
    for location in ["@location(0)", "@location(1)", "@location(2)", "@location(3)"] {
        assert!(
            hand.contains(location),
            "HeldVertexInput is missing {location}; HandVertex declares 4 attributes \
             (position, uv, packed, tint)"
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
    let source = include_str!("../src/engine/shader.wgsl");
    let module = naga::front::wgsl::parse_str(source).expect("chunk shader should parse");

    let names: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
    for wanted in [
        "vs_main",
        "fs_solid",
        "fs_cutout",
        "fs_crack",
        // The models: `LookPipelines` and `ModelReceivers`.
        "vs_item",
        "vs_item_shadowed",
        "fs_cutout_shadowed",
        "vs_actor",
        "fs_actor",
        "fs_actor_shadowed",
        "vs_held",
        "fs_held",
        "fs_held_shadowed",
    ] {
        assert!(
            names.contains(&wanted),
            "shader.wgsl has no entry point {wanted:?}; it has {names:?}"
        );
    }
}

/// The text of a WGSL function, from `fn name(` to the brace that closes it
/// at the start of a line.
fn function_body<'a>(source: &'a str, name: &str) -> &'a str {
    let head = format!("fn {name}(");
    let start = source.find(&head).unwrap_or_else(|| panic!("shader.wgsl has no fn {name}"));
    let rest = &source[start..];
    &rest[..rest.find("\n}").expect("a function that never closes would not compile")]
}

/// **Every model is shaded by the function that shades the ground.**
///
/// The report this is written for: "освещение не работает на 3д модели".
/// Other players and the hand had shaders of their own, each with its own
/// sum for "how lit is this", and when the light became a colour and a
/// lighting step -- all of it in `shade_lit` -- neither followed. The ground
/// went gold at sunset and a figure on it stayed the grey of noon.
///
/// So the property is not "looks right in a picture" but the mechanism: the
/// fragment stage of every pipeline that draws a model reaches `shade_lit_sky`,
/// the one function the lighting step is compiled into, and the shadowed
/// ones reach `shadowed_lambert` as the ground's do. Followed through the
/// helpers they call by name, because that is how the text is written.
///
/// `shade_lit_sky` and not `shade_lit`: the body moved when the shadowed
/// terrain began telling it which sky the sun is scaled by (`sun_sky`), and
/// `shade_lit` is the wrapper that hands it the flood fill's.
#[test]
fn every_model_is_shaded_by_the_function_that_shades_the_ground() {
    let source = include_str!("../src/engine/shader.wgsl");
    fn reaches(source: &str, from: &str, wanted: &str, depth: usize) -> bool {
        let body = function_body(source, from);
        if body.contains(&format!("{wanted}(")) {
            return true;
        }
        depth > 0
            && ["shade", "shade_lit", "actor_colour", "held_surface"]
                .iter()
                .filter(|helper| **helper != from && body.contains(&format!("{helper}(")))
                .any(|helper| reaches(source, helper, wanted, depth - 1))
    }
    for entry in ["fs_cutout", "fs_held", "fs_actor", "fs_cutout_shadowed", "fs_held_shadowed", "fs_actor_shadowed"] {
        assert!(reaches(source, entry, "shade_lit_sky", 4), "{entry} does not reach shade_lit_sky");
    }
    for entry in ["fs_cutout_shadowed", "fs_held_shadowed", "fs_actor_shadowed"] {
        assert!(reaches(source, entry, "shadowed_lambert", 3), "{entry} reads no shadow");
    }
}

/// The `@location`s a stage hands on (a vertex stage's result) or takes (a
/// fragment stage's arguments), struct members included.
fn stage_locations(module: &naga::Module, entry: &str, vertex: bool) -> std::collections::BTreeSet<u32> {
    let point = module
        .entry_points
        .iter()
        .find(|e| e.name == entry)
        .unwrap_or_else(|| panic!("shader.wgsl has no entry point {entry}"));
    let mut found = std::collections::BTreeSet::new();
    let mut add = |ty: naga::Handle<naga::Type>, binding: &Option<naga::Binding>| match binding {
        Some(naga::Binding::Location { location, .. }) => {
            found.insert(*location);
        }
        Some(_) => {}
        None => {
            if let naga::TypeInner::Struct { members, .. } = &module.types[ty].inner {
                for member in members {
                    if let Some(naga::Binding::Location { location, .. }) = member.binding {
                        found.insert(location);
                    }
                }
            }
        }
    };
    if vertex {
        if let Some(result) = &point.function.result {
            add(result.ty, &result.binding);
        }
    } else {
        for argument in &point.function.arguments {
            add(argument.ty, &argument.binding);
        }
    }
    found
}

/// **Every pipeline's fragment stage takes exactly what its vertex stage hands
/// on.**
///
/// Naga validates each entry point on its own, and a vertex stage that hands
/// on a location nothing reads is a valid entry point. wgpu 0.19 is stricter
/// than that: `create_render_pipeline` refuses the pair, and the renderer
/// makes its pipelines at startup -- so the first draft of the models' entry
/// points passed every test in this file and would have ended the game on the
/// frame it opened. It was caught by the offscreen tool, as
/// "Location[10] is provided by the previous stage output but is not consumed
/// as input by this stage", on `actor pipeline`: `vs_actor` hands on the
/// shadow coordinate for the shadowed twin, and the plain `fs_actor` did not
/// take it.
///
/// The pairs are the ones `LookPipelines`, `ModelReceivers` and
/// `ShadowMap::new` build.
#[test]
fn every_pipeline_takes_in_its_fragment_stage_what_its_vertex_stage_hands_on() {
    let source = include_str!("../src/engine/shader.wgsl");
    let module = naga::front::wgsl::parse_str(source).expect("chunk shader should parse");
    for (vertex, fragment) in [
        ("vs_main", "fs_solid"),
        ("vs_main", "fs_cutout"),
        ("vs_main", "fs_crack"),
        ("vs_item", "fs_cutout"),
        ("vs_actor", "fs_actor"),
        ("vs_held", "fs_held"),
        ("vs_main_shadowed", "fs_solid_shadowed"),
        ("vs_main_shadowed", "fs_cutout_shadowed"),
        ("vs_item_shadowed", "fs_cutout_shadowed"),
        ("vs_actor", "fs_actor_shadowed"),
        ("vs_held", "fs_held_shadowed"),
        ("vs_shadow_cutout", "fs_shadow_cutout"),
    ] {
        let handed = stage_locations(&module, vertex, true);
        let taken = stage_locations(&module, fragment, false);
        assert_eq!(
            handed, taken,
            "{vertex} hands on locations {handed:?} and {fragment} takes {taken:?}: \
             wgpu refuses that pipeline"
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
    let source = include_str!("../src/engine/shader.wgsl");
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

/// Every shader's `Globals` has to agree with every other one's, field
/// for field, from the top.
///
/// **A uniform block is an offset table, not a list of names.** A shader
/// that declares only the first six fields is fine -- it reads the first
/// six and ignores the rest -- but one that *skips* a field and declares
/// the next is reading the wrong bytes for everything after it, and
/// nothing anywhere says so: it compiles, it binds, and it draws
/// nonsense. That is exactly what happened when `render_origin` was
/// added between `sky_params` and `hand_view_proj`: the hand shader
/// still listed the two as neighbours and drew the player's arm with a
/// projection matrix made of three quarters of a matrix and a position.
///
/// So each declaration must be a *prefix* of the same sequence. Compared
/// by field name and type, which is all the layout depends on.
#[test]
fn every_shader_reads_the_same_globals() {
    fn fields(source: &str) -> Vec<(String, String)> {
        let Some(start) = source.find("struct Globals {") else {
            return Vec::new();
        };
        let body = &source[start + "struct Globals {".len()..];
        let body = &body[..body.find('}').expect("an unterminated struct would not compile")];
        body.lines()
            .map(|line| line.split("//").next().unwrap_or("").trim())
            .filter(|line| !line.is_empty())
            .map(|line| {
                let line = line.trim_end_matches(',');
                let (name, kind) = line.split_once(':').expect("a field is `name: type`");
                (name.trim().to_string(), kind.trim().to_string())
            })
            .collect()
    }

    // The hand and the other players used to be two more entries here, with
    // Globals blocks of their own; they are entry points of the chunk shader
    // now and read its block.
    let sources: Vec<(&str, &str)> = vec![
        ("shader.wgsl", include_str!("../src/engine/shader.wgsl")),
        ("sky.wgsl", include_str!("../src/engine/sky.wgsl")),
    ];

    // The longest declaration is the reference: it is the only one that
    // can name every field, and any shorter one has to match its start.
    let (longest_name, canonical) = sources
        .iter()
        .map(|(name, src)| (*name, fields(src)))
        .max_by_key(|(_, f)| f.len())
        .expect("there are shaders");
    assert!(canonical.len() >= 8, "{longest_name} declares almost nothing");

    for (name, source) in &sources {
        let declared = fields(source);
        if declared.is_empty() {
            continue; // a shader that does not use the block at all
        }
        for (index, field) in declared.iter().enumerate() {
            assert_eq!(
                field, &canonical[index],
                "{name} field {index} is {field:?}, but {longest_name} has {:?} there -- \
                 every field after it is being read from the wrong offset",
                canonical[index]
            );
        }
    }
}
