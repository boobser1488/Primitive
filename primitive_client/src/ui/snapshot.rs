#![cfg(test)]
//! Draws the interface into a PNG, without a graphics card.
//!
//! ## Why this exists
//!
//! Every screen in this game is built by a pure function: it takes an
//! inventory and a language and answers a list of coloured, textured
//! quads. Nothing about that needs a GPU -- the GPU is only what puts
//! them on a monitor.
//!
//! So the layout can be *looked at* from a terminal, which is the whole
//! point. An interface that can only be reviewed by launching the game,
//! walking to a chest and taking a photograph is an interface that gets
//! reviewed once; one that renders to a file in two seconds is one
//! anybody can put side by side with the last version and argue about.
//!
//! ```text
//! UI_SNAPSHOT_DIR=shots cargo test -p primitive_client --lib \
//!     ui_snapshot -- --ignored --nocapture
//! ```
//!
//! ## What it is not
//!
//! It is not the renderer. Block icons are drawn as flat plates rather
//! than as their textures, because the texture array is built on the
//! graphics card and the thing being reviewed is *the layout*: where
//! the panels are, how much room the text has, what lines up with what.
//! Text is real -- it comes off the same bitmap font the game draws --
//! because most layout mistakes are text mistakes.

use crate::engine::font::{glyph, GLYPH_HEIGHT, GLYPH_WIDTH};
use crate::engine::texture::{FaceLayers, FontAtlas, GLYPHS};
use crate::ui::chest_screen::ChestScreen;
use crate::ui::hotbar::{HotbarVertex, UNTEXTURED};
use crate::ui::inventory_screen::InventoryScreen;
use crate::ui::lang::Language;
use primitive_shared::hearth;
use primitive_shared::inventory::{Inventory, Stack};
use primitive_shared::protocol::{ContainerKind, HearthState, RackState};
use primitive_shared::types::{
    BLOCK_CLAY, BLOCK_COAL, BLOCK_COBBLESTONE, BLOCK_COPPER_ORE, BLOCK_FLINT, BLOCK_STONE_AXE,
    BLOCK_STONE_PICKAXE, BLOCK_LOG, BLOCK_MOULD, BLOCK_PLANKS, BLOCK_RAW_MEAT, BLOCK_STICK,
    BLOCK_VESSEL,
};

/// How big the picture is, and what aspect the game is assumed to be at.
///
/// A 16:9 desktop window unless told otherwise. **Told otherwise is the
/// point**: the layout that had to be fixed is the one on a 2712x1220
/// phone, and the whole value of this harness is being able to look at
/// that shape from a desktop rather than flashing a build to a device
/// and photographing it.
///
/// ```text
/// UI_SNAPSHOT_SIZE=2712x1220 UI_SNAPSHOT_SCALE=1.5 PRIMITIVE_TOUCH_UI=1 \
///   UI_SNAPSHOT_DIR=shots cargo test -p primitive_client --lib \
///   ui_snapshot -- --ignored --nocapture
/// ```
fn size() -> (u32, u32) {
    static SIZE: std::sync::OnceLock<(u32, u32)> = std::sync::OnceLock::new();
    *SIZE.get_or_init(|| {
        let Ok(text) = std::env::var("UI_SNAPSHOT_SIZE") else {
            return (1280, 720);
        };
        let (w, h) = text.split_once('x').expect("a size like 2712x1220");
        (
            w.trim().parse().expect("a width"),
            h.trim().parse().expect("a height"),
        )
    })
}

fn width() -> u32 {
    size().0
}

fn height() -> u32 {
    size().1
}

/// What INTERFACE SIZE is set to for the picture.
fn ui_scale() -> f32 {
    static SCALE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *SCALE.get_or_init(|| {
        std::env::var("UI_SNAPSHOT_SCALE")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(1.0)
    })
}

/// The layout the screens are built against, which is the whole of what
/// this harness varies.
fn snapshot_layout() -> crate::ui::widgets::Layout {
    crate::ui::widgets::Layout::for_screen(width() as f32 / height() as f32, ui_scale())
}

/// Where the interface's own pictures sit in the fixture atlas.
///
/// **The pictures are the point of these PNGs now.** The screens are
/// drawn out of a skin (`widgets::Piece`), and a harness that left the
/// skin off would take a picture of a screen the game does not draw --
/// the one thing this tool must not produce. The numbers are
/// `FaceLayers::empty_for_test`'s own, which start at one and stop well
/// short of the font's base of a thousand, so nothing here can be
/// mistaken for a glyph.
fn snapshot_skin() -> u32 {
    FaceLayers::empty_for_test().extra(crate::engine::texture::EXTRA_UI_SKIN)
}

/// The same, with the skin left off: what these screens looked like
/// before they were drawn out of pictures.
///
/// `UI_SNAPSHOT_SKIN=0` is the "before" of a before-and-after taken in
/// one binary, which is the only kind whose difference is the change --
/// the same argument `PRIMITIVE_GRAIN=0` carries for the ground.
fn wear_the_skin() {
    if std::env::var("UI_SNAPSHOT_SKIN").as_deref() == Ok("0") {
        return;
    }
    crate::ui::widgets::use_skin(snapshot_skin());
}

/// Writes the three screens as PNGs.
///
/// Ignored by default -- it is a tool rather than a check. Point it
/// somewhere with `UI_SNAPSHOT_DIR` and run:
///
/// ```text
/// cargo test -p primitive_client --lib ui_snapshot -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a tool: writes PNGs of the interface for a person to look at"]
fn ui_snapshot() {
    wear_the_skin();
    let out = std::env::var("UI_SNAPSHOT_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");

    // The pack's own resolution, so the picture is packed the way the
    // game packs it. See `blocks.toml`.
    let font = FontAtlas::for_size(32, 1_000);
    let layers = FaceLayers::empty_for_test();
    let pack = a_playing_pack();

    // The hearth, mid-batch: the screen this pass is about.
    let mut hearth_screen = ChestScreen::new();
    hearth_screen.show(
        (0, 0, 0),
        a_working_kiln(),
        Some(primitive_shared::types::BLOCK_KILN_LIT),
        ContainerKind::Hearth(hearth::Kind::Kiln),
        // Climbing through yellow towards the heat a pour needs, so the
        // picture shows the gauge, its line and the colour word at once.
        Some(HearthState {
            fuel_left: 74.0,
            progress: 0.42,
            degrees: 1180.0,
            needs: hearth::COPPER_MELTS_C,
            wet: false,
        }),
        None,
    );
    hearth_screen.set_cursor(Some({
        let slot = crate::ui::chest_screen::hearth_slot_rect(0).expect("an input slot");
        (slot.centre_x(), slot.centre_y())
    }));
    write_grown(
        &format!("{out}/hearth.png"),
        &hearth_screen.build(font, &layers, &pack, Language::English),
        font,
        hearth_screen.grow_by(snapshot_layout()),
    );

    // A chest, for comparison: the screen the hearth is drawn beside.
    let mut chest = ChestScreen::new();
    chest.show(
        (0, 0, 0),
        a_full_chest(),
        Some(primitive_shared::types::BLOCK_CHEST),
        ContainerKind::Chest,
        None,
        None,
    );
    write_grown(
        &format!("{out}/chest.png"),
        &chest.build(font, &layers, &pack, Language::English),
        font,
        chest.grow_by(snapshot_layout()),
    );

    // A dead player's body with a rucksack's compartment, on each of its
    // two pages: the tabs are the only thing a chest does not have.
    for (name, rucksack) in [("corpse", false), ("corpse_rucksack", true)] {
        let mut contents = primitive_shared::inventory::Inventory::body(true);
        for (square, stack) in a_full_chest().slots().iter().enumerate() {
            if let Some(stack) = *stack {
                contents.put_in_slot(square, stack);
            }
        }
        for offset in [0, 3, 11, 19] {
            contents.put_in_slot(
                primitive_shared::inventory::CORPSE_COMPARTMENT.start + offset,
                Stack::new(primitive_shared::types::BLOCK_STONE, 20 + offset as u32),
            );
        }
        let mut body = ChestScreen::new();
        body.show((0, 0, 0), contents, Some(primitive_shared::types::BLOCK_CORPSE), ContainerKind::Chest, None, None);
        if rucksack {
            let tab = crate::ui::chest_screen::page_tab_rect(true);
            body.set_cursor(Some((tab.centre_x(), tab.centre_y())));
            let _ = body.click(&pack, crate::ui::inventory_screen::Button::Left, false, false);
        }
        write_grown(
            &format!("{out}/{name}.png"),
            &body.build(font, &layers, &pack, Language::English),
            font,
            body.grow_by(snapshot_layout()),
        );
    }

    // The rack, part way through a skin. Drawn in the two states worth
    // arguing about: working, and stopped by weather -- the second being
    // the whole reason the screen exists.
    for (name, weather) in [
        (
            "rack",
            RackState { progress: 0.38, rate: 0.85, wet: false, near_fire: false },
        ),
        (
            "rack_rained_off",
            RackState { progress: 0.38, rate: 0.0, wet: true, near_fire: false },
        ),
    ] {
        let mut rack = ChestScreen::new();
        rack.show(
            (0, 0, 0),
            a_loaded_rack(),
            Some(primitive_shared::types::BLOCK_DRYING_RACK),
            ContainerKind::Rack,
            None,
            Some(weather),
        );
        // The hover only on the second one: the note it raises is drawn
        // over the reading above the arrow, and both are worth seeing
        // once.
        if name != "rack" {
            rack.set_cursor(Some({
                let slot = crate::ui::chest_screen::rack_slot_rect(
                    primitive_shared::rack::HIDE_SLOT,
                )
                .expect("a frame");
                (slot.centre_x(), slot.centre_y())
            }));
        }
        // Grown, like the hearth and the chest: a rack drawn at its authored
        // size is a picture of a screen the game never shows anywhere but a
        // desktop at 1.0.
        write_grown(
            &format!("{out}/{name}.png"),
            &rack.build(font, &layers, &pack, Language::English),
            font,
            rack.grow_by(snapshot_layout()),
        );
        // ...and the stopped one in Russian as well. The longest text on
        // any screen in this game is that red line, and a test that
        // measures it against the panel answers "it fits" while a
        // picture answers "it reads".
        if name != "rack" {
            write_grown(
                &format!("{out}/{name}_ru.png"),
                &rack.build(font, &layers, &pack, Language::Russian),
                font,
                rack.grow_by(snapshot_layout()),
            );
        }
    }

    // A jug, looked into in the hand, part full -- and in Russian, whose
    // "what may go in" line is the longest either language has for it.
    // Opened the way the game opens one: from a pack slot, with nothing
    // asked of a server.
    {
        let mut jugged = pack.clone();
        jugged.take_slot(9);
        jugged.put_in_slot(9, primitive_shared::inventory::filled_jug(primitive_shared::types::BLOCK_SEEDS, 11));
        for (name, language) in [("jug", Language::English), ("jug_ru", Language::Russian)] {
            let mut jug = ChestScreen::new();
            jug.show_held_vessel(9, &jugged);
            write_grown(
                &format!("{out}/{name}.png"),
                &jug.build(font, &layers, &jugged, language),
                font,
                jug.grow_by(snapshot_layout()),
            );
        }
    }

    // The gauges, over a stand-in for the bar they sit on.
    //
    // **The hotbar itself is a box here and not the real thing**: its
    // slots draw block icons, which need the texture array, which needs
    // a graphics card. What is being looked at is the *clearance* --
    // whether the strips under the health gauge sit above the bar or on
    // it -- and for that the bar's own outline is the whole of what
    // matters. See `hotbar::TOP`, which is the line they must clear.
    {
        use crate::ui::hotbar::{BOTTOM, PAD, SLOT, UNTEXTURED};
        let mut vertices = Vec::new();
        let mut box_quad = |x0: f32, y0: f32, x1: f32, y1: f32, tint: [f32; 4]| {
            for (x, y) in [
                (x0, y0),
                (x1, y0),
                (x1, y1),
                (x0, y0),
                (x1, y1),
                (x0, y1),
            ] {
                vertices.push(HotbarVertex {
                    position: [x, y],
                    uv: [0.0, 0.0],
                    tex_layer: UNTEXTURED,
                    tint,
                });
            }
        };
        // The backdrop, then the ten slots inside it.
        let pitch = SLOT + 0.012;
        let total = pitch * 10.0 - 0.012;
        box_quad(
            -total / 2.0 - PAD,
            BOTTOM - PAD,
            total / 2.0 + PAD,
            BOTTOM + SLOT + PAD,
            [0.12, 0.12, 0.14, 0.92],
        );
        for slot in 0..10 {
            let centre = crate::ui::hotbar::slot_centre(slot, 10);
            box_quad(
                centre - SLOT / 2.0,
                BOTTOM,
                centre + SLOT / 2.0,
                BOTTOM + SLOT,
                [0.30, 0.31, 0.34, 1.0],
            );
        }
        let mut belt = Inventory::new();
        belt.put_in_slot(0, Stack::new(BLOCK_COBBLESTONE, 41));
        belt.put_in_slot(3, Stack::new(BLOCK_COAL, 7));
        vertices.extend(crate::ui::hud::build(
            font,
            13.0,
            20.0,
            16.0,
            0.55,
            false,
            0.42,
            0.35,
            crate::ui::hud::BodyGauges {
                // A player who has started to get cold but whom the
                // server has not yet called Cold: the drift, which is
                // the state the temperature gauge exists to show and
                // the one that used to draw nothing at all.
                //
                // Derived rather than written down beside the degrees,
                // because the pair used to disagree -- 21 degrees
                // labelled Comfortable is a body the game cannot
                // produce, and a fixture that cannot happen is a
                // picture of nothing.
                temperature_c: 24.0,
                comfort: primitive_shared::body::Comfort::of(24.0),
                hydration: 0.45,
                // Tired enough that the gauge is drawn in the
                // state that matters: past `body::TIRED_AT`,
                // which is where it starts costing something.
                fatigue: 0.82,
                // A cut still bleeding and a leg in a splint: both marks
                // beside the health bar, which is the one place they are
                // drawn and so the one picture that can show whether they
                // collide with anything.
                injuries: snapshot_wounds(),
                // Half soaked and fairly filthy, with the recovery those
                // two cost: the HUD draws none of the three, so what
                // they are for here is the pack screen's health page --
                // and a page of zeroes is a picture that cannot show
                // whether a bad reading is legible.
                wetness: 0.5,
                grime: 0.65,
                recovery: 0.72,
                diet_groups: 2,
                // A cold hut of boards, smoky, with its door in the wind
                // and a smoke hole: every line the place can put on the
                // health page at once, under the wounds, so the picture
                // shows whether they all fit above its floor.
                shelter: primitive_shared::shelter::Reading { air_c: 4.0, indoors: true, draught: 0.5, keeps_out: 0.55, roof_open: true },
                smoke: 0.3,
                downed: None,
            },
            &belt,
            // **A refusal, drawn.** The notice was never in this
            // picture, and that is how its plate came to be printed
            // across the temperature scale: it is placed above the
            // gauges and it was the one piece of the HUD nobody could
            // look at. See `hud::NOTICE_Y`.
            Some(("moved back: too far from the world", 1.0)),
        ));
        write(&format!("{out}/hud.png"), &vertices, font);

        // ...and the same HUD with the first two minutes' line over the
        // belt (`hud::first_step_line`), in Russian, which is the longest
        // of the three wordings. **The clearance is the whole point of the
        // picture**: the line sits between the bar and the gauges of the
        // body, and a plate that overlapped either would be exactly the
        // notice's old mistake one row down.
        {
            let mut p = crate::ui::widgets::Painter::onto(font, vertices.clone());
            crate::ui::hud::first_minute_line(
                &mut p,
                Language::Russian.text(crate::ui::lang::Msg::StepStone),
            );
            write(&format!("{out}/hud_first_minute.png"), &p.into_vertices(), font);
        }
    }

    // The death notice, settled, grown the way the frame loop grows it.
    //
    // **The one screen every player meets that this harness never drew.**
    // It was drawn in a palette of its own -- a plate of hairlines where
    // every other screen is a bevelled slab -- and that kind of drift is
    // invisible from inside one file and obvious the moment the screens
    // are laid side by side, which is the only thing this tool is for.
    // In Russian as well, because `ВЫ ПОГИБЛИ` is the widest title the
    // plate carries.
    for (name, language) in [("death", Language::English), ("death_ru", Language::Russian)] {
        let mut death = crate::ui::death::DeathScreen::new();
        death.open("fell from a great height".to_string());
        for _ in 0..40 {
            death.tick(0.05);
        }
        write_grown(
            &format!("{out}/{name}.png"),
            &death.build(font, language),
            font,
            crate::ui::death::grow_by(snapshot_layout()),
        );
    }

    // The thumb controls on their own: the whole interface a phone
    // has.
    //
    // Worth its own picture now that there is no screen for arranging
    // them. While the editor existed this was visible inside it, which
    // meant the only way to look at the controls was to open the thing
    // that moved them around; with that gone this is the one place the
    // phone's interface can be seen without a phone. See
    // `hud::touch_controls`.
    //
    // **Twice: shut and open.** The three buttons that take a player
    // out of the world live inside the wheel now, and a picture of the
    // glass with it shut does not show them at all -- which is the
    // whole point of the wheel and exactly why it needs its own
    // picture. `touch_controls_wheel.png` is where the arc is checked
    // for reaching off the glass or into the hotbar.
    for wheel_open in [false, true] {
        let mut painter = crate::ui::widgets::Painter::new(font);
        let controls = crate::platform::touch::Layout::for_size(
            crate::platform::Size::new(width(), height()),
            crate::settings::TouchLayout::default(),
            1.5,
            wheel_open,
        );
        // Half of them drawn as held, so a picture shows both faces a
        // button has -- the pale pressed one is where the label had to
        // change colour. See `a_thumb_button_can_be_read_against_the_world_behind_it`.
        crate::ui::hud::touch_controls(&mut painter, &controls, |slot| slot % 2 == 1, Language::English);
        let mut vertices = painter.into_vertices();

        // ...and the hotbar beneath them, **grown the way the game
        // grows it**, because that relationship is the one that was
        // wrong: the bar answers to INTERFACE SIZE and the buttons do
        // not, so a picture of the buttons alone says nothing about
        // whether a slot is reachable. Drawn as outlines rather than
        // slots -- what is being looked at is the clearance.
        {
            use crate::ui::hotbar::{BOTTOM, PAD, SLOT, UNTEXTURED};
            let mut bar = Vec::new();
            let mut box_quad = |x0: f32, y0: f32, x1: f32, y1: f32, tint: [f32; 4]| {
                for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y0), (x1, y1), (x0, y1)] {
                    bar.push(HotbarVertex { position: [x, y], uv: [0.0, 0.0], tex_layer: UNTEXTURED, tint });
                }
            };
            let pitch = SLOT + 0.012;
            let total = pitch * 10.0 - 0.012;
            box_quad(-total / 2.0 - PAD, BOTTOM - PAD, total / 2.0 + PAD, BOTTOM + SLOT + PAD,
                     [0.12, 0.12, 0.14, 0.92]);
            for slot in 0..10 {
                let centre = crate::ui::hotbar::slot_centre(slot, 10);
                box_quad(centre - SLOT / 2.0, BOTTOM, centre + SLOT / 2.0, BOTTOM + SLOT,
                         [0.30, 0.31, 0.34, 1.0]);
            }
            crate::ui::widgets::scale_about(
                &mut bar,
                crate::ui::widgets::anchor::BOTTOM(width() as f32 / height() as f32),
                ui_scale(),
            );
            vertices.extend(bar);
        }
        let name = if wheel_open {
            "touch_controls_wheel"
        } else {
            "touch_controls"
        };
        write(&format!("{out}/{name}.png"), &vertices, font);
    }

    // ---- every page of the pack, in both languages, with the worst
    // content it can be asked to hold ----
    //
    // **Three tabs and only one of them had ever been drawn here.** The
    // body page and the rucksack page are the two the player reported
    // text running over things on, and neither was in this folder: the
    // pack page was rendered twice, in two languages, and the other two
    // not at all. A harness that draws one of three pages is a harness
    // that says nothing about the screen.
    //
    // `a_stuffed_pack` is the content that breaks layouts rather than
    // the content a tidy player has: every square full, counts in three
    // digits, and the longest item names either language owns
    // ("Кирпичная кладка на растворе" is twenty-eight characters, and
    // the slot it sits in is about four wide).
    {
        use crate::ui::inventory_screen::Tab;
        let stuffed = a_stuffed_pack();
        let equipment = a_dressed_body();
        let vitals = snapshot_vitals();
        let scale = crate::ui::inventory_screen::grow_by(snapshot_layout());
        // A player part way up: the copper age reached, bronze next, so
        // the path page has lit rungs, dim rungs and a rung that wants
        // something the player has never held -- which is every state a
        // rung can be in, in one picture.
        let knows = primitive_shared::discovery::Discovered::from_kinds([
            primitive_shared::types::BLOCK_PEBBLE,
            primitive_shared::types::BLOCK_FLINT_FLAKE,
            primitive_shared::types::BLOCK_CAMPFIRE,
            primitive_shared::types::BLOCK_KILN,
            primitive_shared::types::BLOCK_COPPER_INGOT,
            primitive_shared::types::BLOCK_VESSEL,
        ]);
        let keys = crate::ui::keybinds::Keybinds::default();
        let learning = crate::ui::ladder_screen::Learning { discovered: &knows, keys: &keys };
        for (tab, stem) in [
            (Tab::Health, "pack_body"),
            (Tab::Pack, "pack_things"),
            (Tab::Backpack, "pack_rucksack"),
            (Tab::Learn, "pack_path"),
        ] {
            for (language, tag) in [(Language::English, ""), (Language::Russian, "_ru")] {
                let mut screen = InventoryScreen::new();
                screen.open = true;
                screen.sync(&stuffed);
                screen.set_tab(tab);
                let mut vertices = Vec::new();
                screen.build_into(
                    font,
                    &layers,
                    &stuffed,
                    &equipment,
                    &snapshot_wounds(),
                    &vitals,
                    learning,
                    language,
                    &mut vertices,
                );
                write_grown(&format!("{out}/{stem}{tag}.png"), &vertices, font, scale);
            }
        }
    }

    // ...and the pack, which is the screen a player is in most.
    let mut inventory = InventoryScreen::new();
    inventory.open = true;
    inventory.sync(&pack);
    let pack_scale = crate::ui::inventory_screen::grow_by(snapshot_layout());
    write_grown(
        &format!("{out}/inventory.png"),
        &inventory.build(font, &layers, &pack, 0.72, Language::English),
        font,
        pack_scale,
    );

    // ...and the pack in Russian, which is where the longest words in
    // the game are: a recipe row is a name, a status and a count on one
    // line, and "нужно" is half again as long as "need".
    let mut russian = InventoryScreen::new();
    russian.open = true;
    russian.sync(&pack);
    write_grown(
        &format!("{out}/inventory_ru.png"),
        &russian.build(font, &layers, &pack, 0.72, Language::Russian),
        font,
        pack_scale,
    );

    // ...and the pack with a recipe under the pointer, which is where
    // the longest text on any of these screens ends up: a tooltip is
    // drawn wherever the cursor is, and the cursor can be at the edge.
    let mut hovered = InventoryScreen::new();
    hovered.open = true;
    hovered.sync(&pack);
    let row = crate::ui::inventory_screen::recipe_rect(0, 0);
    hovered.set_cursor(Some((row.centre_x(), row.centre_y())));
    write_grown(
        &format!("{out}/inventory_tooltip.png"),
        &hovered.build(font, &layers, &pack, 0.72, Language::English),
        font,
        pack_scale,
    );

    // The two station screens: the anvil at rest with its list of jobs, and
    // the wheel mid-run, so the bar, the sweet spot, the marker and the pips
    // are all in one picture. Mid-run at a moment picked so the marker is not
    // sitting at either end of the bar -- a marker at zero is a screenshot of
    // a screen that has not started.
    {
        use crate::ui::station_screen::{jobs_of, StationScreen};
        use primitive_shared::minigame::{tolerance, Game};
        let mut anvil = StationScreen::new();
        anvil.asked_to_open();
        anvil.show(Game::Anvil, tolerance(Some(primitive_shared::types::BLOCK_STONE_HAMMER)));
        // Hovering the first row, so the button's lit state is in the picture.
        let row = crate::ui::station_screen::Panel::for_game(Game::Anvil).row(0);
        anvil.set_cursor(Some((row.centre_x(), row.centre_y())));
        let mut out_v = Vec::new();
        anvil.build_into(font, &layers, Language::English, &mut out_v);
        write_grown(&format!("{out}/anvil.png"), &out_v, font, anvil.grow_by(snapshot_layout()));

        // ...and the wheel's list in Russian, whose longest name has to clear
        // the picture beside it.
        let mut wheel_jobs = StationScreen::new();
        wheel_jobs.asked_to_open();
        wheel_jobs.show(Game::Wheel, tolerance(None));
        let mut out_v = Vec::new();
        wheel_jobs.build_into(font, &layers, Language::Russian, &mut out_v);
        write_grown(&format!("{out}/wheel.png"), &out_v, font, wheel_jobs.grow_by(snapshot_layout()));

        let job = jobs_of(Game::Wheel)[0];
        let wheel = StationScreen::mid_run(Game::Wheel, tolerance(None), job, 0x5EED, 900);
        let mut out_v = Vec::new();
        wheel.build_into(font, &layers, Language::Russian, &mut out_v);
        write_grown(&format!("{out}/wheel_run.png"), &out_v, font, wheel.grow_by(snapshot_layout()));
    }

    // ...and a body that has been in a fight: a cut still bleeding, a leg
    // already splinted, a bruise and a light burn -- every colour and every
    // mark the figure has. Twice: with a bandage in hand over the bleeding
    // arm, so the parts it would help are lit; and with empty hands on the
    // same arm, so the tooltip says what it needs. In Russian, because the
    // wound lines are the longest text a tooltip on this screen carries.
    let mut dressing = pack.clone();
    dressing.add(primitive_shared::types::BLOCK_BANDAGE, 3);
    let bandage = (0..primitive_shared::inventory::SLOTS)
        .find(|&s| dressing.block_in(s) == Some(primitive_shared::types::BLOCK_BANDAGE))
        .expect("the bandage went in");
    let arm = crate::ui::mannequin::part_rect(
        crate::ui::inventory_screen::mannequin_rect(),
        primitive_shared::injury::Part::LeftArm,
    );
    for (name, carrying) in [("inventory_wounds_dressing", true), ("inventory_wounds", false)] {
        let mut wounded = InventoryScreen::new();
        wounded.open = true;
        wounded.sync(&dressing);
        if carrying {
            let slot = crate::ui::inventory_screen::slot_rect(bandage);
            wounded.set_cursor(Some((slot.centre_x(), slot.centre_y())));
            let _ = wounded.click(&dressing, crate::ui::inventory_screen::Button::Left, false);
        }
        wounded.set_cursor(Some((arm.centre_x(), arm.centre_y())));
        write_grown(
            &format!("{out}/{name}.png"),
            &wounded.build_wounded(font, &layers, &dressing, &snapshot_wounds(), Language::Russian, 0.72),
            font,
            pack_scale,
        );
    }

    // ...and the menu's own screens, which wear the other skin.
    //
    // **In every language**, and the settings screen most of all: it is
    // the one with a label and a reading on the same line, and Russian
    // and Polish spell both of them longer than English does. A screen
    // that has only ever been looked at in English is a screen whose
    // layout has only ever been checked against the shortest words it
    // will ever hold.
    let mut settings = crate::settings::ClientSettings::default();
    let worlds = crate::logic::worlds::Worlds::load(std::path::Path::new("saves"));
    let mut menu = crate::ui::menu::Menu::new(crate::ui::menu::ServerList::default());
    // The extensions screen is empty on a client that has not asked a
    // server anything, and empty is exactly the state that needs no
    // checking. Seeded with a list a small server would really answer --
    // one plugin, two mods, one of them stopped -- because what has to
    // be looked at is a long description beside a short name, settings
    // under it, and whether any of it runs off the pane.
    menu.set_extensions(a_servers_extensions());
    for language in Language::ALL.iter().copied() {
        settings.language = language;
        let tag = match language {
            Language::English => "",
            Language::SimpleEnglish => "_simple",
            Language::Russian => "_ru",
            Language::Polish => "_pl",
        };
        for (screen, name) in [
            (crate::ui::menu::Screen::Main, "menu_main"),
            (crate::ui::menu::Screen::Worlds, "menu_worlds"),
            (crate::ui::menu::Screen::Settings, "menu_settings"),
            (crate::ui::menu::Screen::CreatingWorld, "menu_new_world"),
            (crate::ui::menu::Screen::Servers, "menu_servers"),
            (crate::ui::menu::Screen::Paused, "menu_paused"),
            (crate::ui::menu::Screen::Editing(None), "menu_server_form"),
            (crate::ui::menu::Screen::Credits, "menu_credits"),
            (crate::ui::menu::Screen::Extensions, "menu_extensions"),
        ] {
            // Only the settings screen in the other three: it is the one
            // whose rows can collide, and four times four pictures is a
            // folder nobody looks through.
            if !tag.is_empty() && screen != crate::ui::menu::Screen::Settings {
                continue;
            }
            menu.screen = screen;
            let context = crate::ui::menu::MenuContext {
                version: "1.5.0",
                font,
                settings: &settings,
                worlds: &worlds,
                background: backdrop(),
                layout: snapshot_layout(),
            };
            let mut vertices = Vec::new();
            menu.build_into(&context, &mut vertices);
            write(&format!("{out}/{name}{tag}.png"), &vertices, font);
        }

        // **The extensions screen with nothing on it, which is the one
        // an Android player gets and the one nobody had looked at.**
        // The phone build asks for no native loader and no scripting
        // engine -- see `Menu::this_build_loads_mods` -- so this screen
        // is a panel and one sentence, and that sentence is longer than
        // the one a desktop reads. Whether it wraps inside the panel is
        // a question about a picture, not about a string length.
        menu.set_extensions(primitive_shared::protocol::ExtensionList {
            native_api: None,
            scripts_supported: false,
            items: Vec::new(),
        });
        menu.screen = crate::ui::menu::Screen::Extensions;
        let context = crate::ui::menu::MenuContext {
            version: "1.5.0",
            font,
            settings: &settings,
            worlds: &worlds,
            background: backdrop(),
            layout: snapshot_layout(),
        };
        let mut vertices = Vec::new();
        menu.build_into(&context, &mut vertices);
        write(&format!("{out}/menu_extensions_none{tag}.png"), &vertices, font);
        menu.set_extensions(a_servers_extensions());
    }

    println!("wrote the screens to {out}");
}

/// What a small server with a few things installed would answer.
///
/// Written out here rather than loaded, because the point of a snapshot
/// is a picture that is the same every time it is taken -- a list read
/// off whatever is in a `mods/` folder would make this a picture of the
/// machine it was rendered on.
fn a_servers_extensions() -> primitive_shared::protocol::ExtensionList {
    use primitive_shared::protocol::{ExtensionInfo, ExtensionKind, ExtensionList};
    let entry = |kind, name: &str, version: &str, description: &str, authors: &[&str]| {
        ExtensionInfo {
            kind,
            name: name.to_string(),
            version: version.to_string(),
            description: description.to_string(),
            authors: authors.iter().map(|a| a.to_string()).collect(),
            enabled: true,
            reason: String::new(),
            built_for: matches!(kind, ExtensionKind::Native).then_some((2, 1)),
            settings: Vec::new(),
        }
    };
    ExtensionList {
        native_api: Some((2, 1)),
        scripts_supported: true,
        items: vec![
            entry(
                ExtensionKind::Script,
                "greeter",
                "0.2.0",
                "Says something to everyone who joins, and remembers who has been here before.",
                &["someone"],
            ),
            ExtensionInfo {
                settings: vec![
                    ("tunnel_width".to_string(), "0.09".to_string()),
                    ("rooms".to_string(), "true".to_string()),
                    ("seeded".to_string(), "false".to_string()),
                ],
                ..entry(
                    ExtensionKind::Native,
                    "bigger_caves",
                    "1.2.0",
                    "Wider tunnels and more of them, with rooms where three meet.",
                    &["someone", "somebody else"],
                )
            },
            ExtensionInfo {
                enabled: false,
                reason: "stopped after 32 error(s)".to_string(),
                ..entry(
                    ExtensionKind::Native,
                    "flight",
                    "0.1.0",
                    "Grants flight to operators.",
                    &["Anthropic"],
                )
            },
        ],
    }
}

/// What somebody who has been playing for an hour is carrying.
fn a_playing_pack() -> Inventory {
    let mut pack = Inventory::new();
    pack.put_in_slot(0, Stack::worn(BLOCK_STONE_PICKAXE, 1, 61));
    pack.put_in_slot(1, Stack::worn(BLOCK_STONE_AXE, 1, 12));
    pack.put_in_slot(2, Stack::new(BLOCK_COBBLESTONE, 41));
    pack.put_in_slot(3, Stack::new(BLOCK_LOG, 6));
    pack.put_in_slot(4, Stack::new(BLOCK_COAL, 12));
    pack.put_in_slot(6, Stack::new(BLOCK_RAW_MEAT, 3));
    pack.put_in_slot(9, Stack::new(BLOCK_FLINT, 2));
    pack.put_in_slot(10, Stack::new(BLOCK_PLANKS, 128));
    pack.put_in_slot(11, Stack::new(BLOCK_STICK, 9));
    pack.put_in_slot(14, Stack::new(BLOCK_CLAY, 7));
    pack.put_in_slot(23, Stack::new(BLOCK_COPPER_ORE, 18));
    pack
}

/// The pack that breaks layouts: every square full, three-digit counts,
/// a worn rucksack with things in it, and the longest names either
/// language has.
///
/// **Deliberately worse than a real pack.** The tidy fixture above is
/// what an hour of play looks like and it is exactly the content a
/// layout mistake hides behind: eleven full squares out of forty, no
/// count over 128, and every name short. The player's complaint was
/// about text running over things, and text only runs over things when
/// there is text.
fn a_stuffed_pack() -> Inventory {
    use primitive_shared::types::*;
    let mut pack = a_playing_pack();
    // Long names first, in the belt, where the tooltip has the panel's
    // own floor under it and the least room to open downwards.
    pack.put_in_slot(5, Stack::new(BLOCK_BRICK_COURSES, 999));
    pack.put_in_slot(7, Stack::new(BLOCK_PEGGED_BIRCH_PLANKS, 640));
    pack.put_in_slot(8, Stack::worn(BLOCK_COPPER_PICKAXE, 1, 3));
    // ...then everything else, so no square is empty: an empty square
    // draws no count and no wear bar, and the squares that draw
    // nothing are the ones a layout mistake hides in.
    let filler = [
        BLOCK_COBBLESTONE, BLOCK_LOG, BLOCK_PLANKS, BLOCK_COAL, BLOCK_CLAY,
        BLOCK_FLINT, BLOCK_STICK, BLOCK_COPPER_ORE, BLOCK_SAND, BLOCK_VESSEL,
    ];
    for slot in 0..primitive_shared::inventory::SLOTS {
        if pack.slots()[slot].is_none() {
            let block = filler[slot % filler.len()];
            pack.put_in_slot(slot, Stack::new(block, 100 + (slot as u32 * 37) % 800));
        }
    }
    // The rucksack's squares, opened the way the server opens them, so
    // the third tab has something on it.
    pack.open_backpack();
    for square in 0..primitive_shared::inventory::BACKPACK_SLOTS {
        pack.put_in_slot(
            primitive_shared::inventory::SLOTS + square,
            Stack::new(filler[square % filler.len()], 7 + square as u32 * 61),
        );
    }
    pack
}

/// A body with something on every square, for the pictures of the worn
/// row: four ghosts is a picture of the empty case, which the tidy
/// fixture already covers.
fn a_dressed_body() -> primitive_shared::inventory::Equipment {
    use primitive_shared::types::*;
    let mut worn = primitive_shared::inventory::Equipment::new();
    for block in [BLOCK_WOOL_TUNIC, BLOCK_RUCKSACK] {
        worn.wear(Stack::new(block, 1));
    }
    worn
}

/// A body in the state the health page exists to describe: hurt, thirsty,
/// tired, soaked, filthy, in a cold draughty hut with a hole in the roof.
///
/// Every row of the page reading something other than 100%, because a
/// page of green hundreds is a picture in which no number is wide.
fn snapshot_vitals() -> crate::ui::inventory_screen::Vitals {
    crate::ui::inventory_screen::Vitals {
        health: 0.34,
        nourishment: 0.22,
        stamina: 0.61,
        body: crate::ui::hud::BodyGauges {
            temperature_c: 24.0,
            comfort: primitive_shared::body::Comfort::of(24.0),
            hydration: 0.45,
            fatigue: 0.82,
            injuries: snapshot_wounds(),
            wetness: 0.5,
            grime: 0.65,
            recovery: 0.72,
            diet_groups: 2,
            shelter: primitive_shared::shelter::Reading {
                air_c: 4.0,
                indoors: true,
                draught: 0.5,
                keeps_out: 0.55,
                roof_open: true,
            },
            smoke: 0.3,
            downed: None,
        },
    }
}

/// A kiln part way through a pour.
fn a_working_kiln() -> Inventory {
    let mut fire = Inventory::new();
    fire.put_in_slot(0, Stack::new(BLOCK_COPPER_ORE, 6));
    fire.put_in_slot(1, Stack::new(BLOCK_COAL, 3));
    fire.put_in_slot(2, Stack::new(BLOCK_VESSEL, 1));
    fire.put_in_slot(3, Stack::new(BLOCK_MOULD, 1));
    fire.put_in_slot(hearth::FUEL_SLOT, Stack::new(BLOCK_COAL, 8));
    fire.put_in_slot(
        hearth::OUTPUT_SLOTS.start,
        Stack::new(primitive_shared::types::BLOCK_COPPER_INGOT, 2),
    );
    fire
}

/// A rack with skins on the frame and a piece already cured.
fn a_loaded_rack() -> Inventory {
    let mut rack = Inventory::new();
    rack.put_in_slot(
        primitive_shared::rack::HIDE_SLOT,
        Stack::new(primitive_shared::types::BLOCK_HIDE, 3),
    );
    rack.put_in_slot(
        primitive_shared::rack::LEATHER_SLOT,
        Stack::new(primitive_shared::types::BLOCK_LEATHER, 1),
    );
    rack
}

fn a_full_chest() -> Inventory {
    let mut chest = Inventory::new();
    for (slot, block) in [
        BLOCK_COBBLESTONE,
        BLOCK_LOG,
        BLOCK_PLANKS,
        BLOCK_COAL,
        BLOCK_CLAY,
    ]
    .into_iter()
    .enumerate()
    {
        chest.put_in_slot(slot, Stack::new(block, 64));
        chest.put_in_slot(slot + 12, Stack::new(block, 7));
    }
    chest
}

/// What the menu screens are drawn over.
///
/// **A still, and only for looking at the interface.** What the player
/// sees behind the menu is a live scene: real chunk meshes, redrawn
/// every frame by the same pipeline that draws the world, under a
/// camera that slowly turns -- see `logic::menu_scene` and the menu
/// branch of the frame. Nothing in the game shows a PNG. This is a
/// still of that scene, stood under the interface so that a layout
/// argument can be settled from a terminal.
///
/// **This harness cannot render a world, and it should not learn how.**
/// It exists because every screen in the game is a pure function from
/// state to quads, and that needs no graphics card; a world is the one
/// thing in the game that genuinely does. So the two halves are done by
/// the two tools that can do them, and joined here:
///
/// ```text
/// GPU_REPRO_DIR=shots cargo test -p primitive_client --lib \
///     the_menu_backdrop_through_the_real_shader -- --ignored --nocapture
/// UI_SNAPSHOT_WORLD=shots/menu_backdrop_shore.png \
/// UI_SNAPSHOT_SIZE=1280x720 UI_SNAPSHOT_DIR=shots \
///     cargo test -p primitive_client --lib ui_snapshot -- --ignored --nocapture
/// ```
///
/// The result is the real interface over the real backdrop, veil and
/// all -- which is the only picture that answers "can this be read",
/// because the veil is *in* the menu's own quads and the thing it is
/// veiling is in the PNG. Without the variable nothing changes: the
/// screens are drawn over the flat ground they always were.
fn backdrop() -> crate::ui::menu::Backdrop {
    match std::env::var("UI_SNAPSHOT_WORLD") {
        // Which place it is decides which veil the menu draws, and the
        // file the picture came from is named after it -- so the name
        // is where it is read from rather than being a second variable
        // that can disagree with the first.
        Ok(path) if path.contains("cave") => {
            crate::ui::menu::Backdrop::Scene(crate::logic::menu_scene::Place::Cave)
        }
        Ok(_) => crate::ui::menu::Backdrop::Scene(crate::logic::menu_scene::Place::Shore),
        Err(_) => crate::ui::menu::Backdrop::Bare,
    }
}

/// The pixels a screen starts from: a rendered world if one was named,
/// and otherwise the flat grey this harness has always used.
///
/// Scaled to the picture's size with nearest-neighbour, so a backdrop
/// rendered at one shape can still be looked at under a menu laid out
/// for another -- a phone's, for instance, which is the shape the
/// layout arguments are usually about.
///
/// One honest caveat, and it errs the safe way: `fill` blends in sRGB
/// bytes, while the card blends the real veil in linear light. So the
/// backdrop reads *darker* here than it does on a screen. A menu that
/// is legible in this picture is legible in the game; a backdrop that
/// looks lost in it may not be. The numbers that decide the veil come
/// from the GPU tool, not from here -- see `menu::SCENE_HIGHLIGHT_OUTDOORS`.
fn ground() -> Vec<[u8; 4]> {
    // A mid-grey ground, so a panel that is nearly the same colour as
    // the background shows up as the problem it is.
    let flat = || vec![[70u8, 78, 92, 255]; (width() * height()) as usize];
    let Ok(path) = std::env::var("UI_SNAPSHOT_WORLD") else {
        return flat();
    };
    let Ok(image) = image::open(&path) else {
        println!("could not read {path}; the screens are over the flat ground");
        return flat();
    };
    let image = image.to_rgba8();
    let (w, h) = (image.width().max(1), image.height().max(1));
    (0..height())
        .flat_map(|y| {
            (0..width()).map(move |x| {
                let sx = (x as u64 * w as u64 / width() as u64).min(w as u64 - 1) as u32;
                let sy = (y as u64 * h as u64 / height() as u64).min(h as u64 - 1) as u32;
                (sx, sy)
            })
        })
        .map(|(sx, sy)| image.get_pixel(sx, sy).0)
        .collect()
}

// ---- the rasteriser ----

/// A body that has been in a fight, for the pictures that show wounds:
/// one of each kind, one of them dressed, so every colour and mark the
/// figure and the HUD have is in a picture somewhere.
fn snapshot_wounds() -> primitive_shared::injury::Injuries {
    use primitive_shared::injury::{Injuries, Kind, Part};
    let mut body = Injuries::default();
    body.inflict(Part::LeftArm, Kind::Cut, 0.7);
    body.inflict(Part::RightLeg, Kind::Fracture, 0.8);
    let _ = body.treat(Part::RightLeg, primitive_shared::types::BLOCK_SPLINT);
    body.inflict(Part::Torso, Kind::Bruise, 0.5);
    body.inflict(Part::LeftLeg, Kind::Burn, 0.25);
    body
}

/// The same, for a screen the frame loop grows before drawing.
///
/// The pack, a chest and the death notice are built at their authored
/// size and multiplied about the middle of the window -- see
/// `inventory_screen::grow_by`. A picture of one that skipped that step
/// would be a picture of a screen the game does not draw, which is the
/// one thing this harness must not produce.
fn write_grown(path: &str, vertices: &[HotbarVertex], font: FontAtlas, scale: f32) {
    let mut grown = vertices.to_vec();
    crate::ui::widgets::scale_about(&mut grown, (0.0, 0.0), scale);
    write(path, &grown, font);
}

/// Turns the quads into pixels.
///
/// Two triangles a quad, and every quad in these screens is
/// axis-aligned, so this fills rectangles rather than rasterising
/// triangles: the corners of each six-vertex run are its bounds.
fn write(path: &str, vertices: &[HotbarVertex], font: FontAtlas) {
    let mut pixels = ground();
    let glyphs = glyph_lookup(font);
    let skin = crate::ui::widgets::skin();

    for quad in vertices.chunks_exact(6) {
        let xs = quad.iter().map(|v| v.position[0]);
        let ys = quad.iter().map(|v| v.position[1]);
        let (x0, x1) = (
            xs.clone().fold(f32::MAX, f32::min),
            xs.fold(f32::MIN, f32::max),
        );
        let (y0, y1) = (
            ys.clone().fold(f32::MAX, f32::min),
            ys.fold(f32::MIN, f32::max),
        );
        let tint = quad[0].tint;

        // **Two triangles that are not a box.** Every widget is a box, and
        // this used to be the whole of what was drawn; the map's arrows are
        // triangles (see `map_screen::arrow`), and filled as their bounds
        // they came out as black squares -- a picture of a map with no way
        // to tell which way anybody was facing.
        if quad[0].tex_layer == UNTEXTURED && !is_a_box(quad, (x0, y0, x1, y1)) {
            for triangle in quad.chunks_exact(3) {
                fill_flat_triangle(
                    &mut pixels,
                    [
                        (triangle[0].position[0], triangle[0].position[1]),
                        (triangle[1].position[0], triangle[1].position[1]),
                        (triangle[2].position[0], triangle[2].position[1]),
                    ],
                    triangle[0].tint,
                );
            }
            continue;
        }

        // The interface's own pictures, drawn as the pictures they are.
        // See `fill_picture`.
        if let Some(piece) = skin.piece_of(quad[0].tex_layer) {
            let us = quad.iter().map(|v| v.uv[0]);
            let vs = quad.iter().map(|v| v.uv[1]);
            let (u0, u1) = (
                us.clone().fold(f32::MAX, f32::min),
                us.fold(f32::MIN, f32::max),
            );
            let (v0, v1) = (
                vs.clone().fold(f32::MAX, f32::min),
                vs.fold(f32::MIN, f32::max),
            );
            fill_picture(
                &mut pixels,
                skin_picture(piece),
                (x0, y0, x1, y1),
                (u0, v0, u1, v1),
                tint,
            );
            continue;
        }

        match quad[0].tex_layer {
            UNTEXTURED => fill(&mut pixels, x0, y0, x1, y1, tint, 1.0),
            // A glyph quad's corner is its *smallest* uv; the first
            // vertex carries the bottom-left one, whose v is the far
            // edge. Taking the minimum over the quad is what makes this
            // agree with `FontAtlas::place` whichever corner comes
            // first.
            layer => match glyphs.get(&(
                layer,
                key(quad.iter().map(|v| v.uv[0]).fold(f32::MAX, f32::min)),
                key(quad.iter().map(|v| v.uv[1]).fold(f32::MAX, f32::min)),
            )) {
                // Real text, off the same font the game draws with.
                Some(&c) => draw_glyph(&mut pixels, c, x0, y0, x1, y1, tint),
                // A block icon: a flat plate. What is being reviewed is
                // where it sits, not what it is a picture of.
                //
                // **Through its own tint, which it did not use to be.**
                // The plate was one hardcoded blue-grey at full opacity
                // whatever the quad carried, and the shader multiplies
                // the picture by the tint -- so every garment came out
                // the colour of every stone (`types::garment_tint` is
                // how twelve garments share four pictures), and the
                // faint ghost in an empty armour square came out
                // pixel-identical to a solid item lying in it. That is
                // the one thing a picture of the pack screen is taken to
                // check, and the picture was answering it wrongly.
                None => {
                    let plate = [
                        0.36 * tint[0],
                        0.44 * tint[1],
                        0.52 * tint[2],
                        tint[3],
                    ];
                    let lip = [0.5 * tint[0], 0.6 * tint[1], 0.7 * tint[2], tint[3]];
                    fill(&mut pixels, x0, y0, x1, y1, plate, 1.0);
                    fill(&mut pixels, x0, y0, x1, y0 + 0.004, lip, 1.0);
                }
            },
        }
    }

    save_png(path, &pixels);
}

/// Whether six vertices are the corners of the box they span.
fn is_a_box(quad: &[HotbarVertex], (x0, y0, x1, y1): (f32, f32, f32, f32)) -> bool {
    let on = |v: f32, a: f32, b: f32| (v - a).abs() < 1e-5 || (v - b).abs() < 1e-5;
    quad.iter().all(|v| on(v.position[0], x0, x1) && on(v.position[1], y0, y1))
}

/// Fills one flat triangle in interface coordinates, blended by its
/// alpha, whichever way it is wound.
fn fill_flat_triangle(pixels: &mut [[u8; 4]], points: [(f32, f32); 3], tint: [f32; 4]) {
    let aspect = width() as f32 / height() as f32;
    let to_pixel = |(x, y): (f32, f32)| {
        (
            (x / aspect + 1.0) * 0.5 * width() as f32,
            (1.0 - (y + 1.0) * 0.5) * height() as f32,
        )
    };
    let p = points.map(to_pixel);
    let area = edge(p[0], p[1], p[2]);
    if area.abs() < 1e-6 {
        return;
    }
    let min_x = p.iter().map(|q| q.0).fold(f32::MAX, f32::min).floor().max(0.0) as u32;
    let max_x = (p.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil() as i64).clamp(0, width() as i64) as u32;
    let min_y = p.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor().max(0.0) as u32;
    let max_y = (p.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil() as i64).clamp(0, height() as i64) as u32;
    let alpha = tint[3].clamp(0.0, 1.0);
    for y in min_y..max_y {
        for x in min_x..max_x {
            let at = (x as f32 + 0.5, y as f32 + 0.5);
            let inside = [edge(p[1], p[2], at), edge(p[2], p[0], at), edge(p[0], p[1], at)]
                .iter()
                .all(|w| w / area >= 0.0);
            if !inside {
                continue;
            }
            let index = (y * width() + x) as usize;
            let under = pixels[index];
            for channel in 0..3 {
                let over = tint[channel].clamp(0.0, 1.0) * 255.0;
                pixels[index][channel] = (under[channel] as f32 * (1.0 - alpha) + over * alpha).round() as u8;
            }
        }
    }
}

/// Draws the journal -- the map and the recipe book.
///
/// Over real terrain: the generator's own chunks, surveyed exactly the
/// way the game surveys the chunks a server streams, because a map of a
/// made-up checkerboard answers nothing about whether a coast reads as a
/// coast.
///
/// ```text
/// UI_SNAPSHOT_DIR=shots cargo test -p primitive_client --lib \
///     journal_snapshot -- --ignored --nocapture
/// UI_SNAPSHOT_SIZE=2712x1220 PRIMITIVE_TOUCH_UI=1 UI_SNAPSHOT_DIR=shots \
///     cargo test -p primitive_client --lib journal_snapshot -- --ignored --nocapture
/// ```
/// Pictures of finding the way: the compass and the sky's line on the
/// view, the chat box asking a cairn's name over a phone's keyboard, and
/// the map with named cairns on it.
///
/// ```text
/// UI_SNAPSHOT_SIZE=2712x1220 UI_SNAPSHOT_SCALE=1.5 PRIMITIVE_TOUCH_UI=1 \
///   UI_SNAPSHOT_DIR=shots/nav cargo test -p primitive_client --lib nav_snapshot -- --ignored
/// ```
#[test]
#[ignore = "a tool: writes PNGs of the way-finding pieces for a person to look at"]
fn nav_snapshot() {
    wear_the_skin();
    use crate::logic::map::{Ground, Tile};
    use crate::ui::journal::{Journal, Tab};
    use crate::ui::lang::Msg;
    use crate::ui::map_screen::PlayerMark;
    use crate::ui::widgets::{anchor, scale_about, Painter};
    use primitive_shared::types::ChunkPos;

    let out = std::env::var("UI_SNAPSHOT_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let touch = crate::ui::widgets::touch_layout();
    let tag = if touch { "_touch" } else { "" };
    let font = FontAtlas::for_size(32, 1_000);
    let aspect = width() as f32 / height() as f32;
    let layout = snapshot_layout();

    // The top of the view: a compass in the hand, facing east, and the sky
    // read off the dawn sun -- both at once, which is the crowded case.
    for (language, lang) in [(Language::English, ""), (Language::Russian, "_ru"), (Language::Polish, "_pl")] {
        let mut p = Painter::onto(font, Vec::new());
        crate::ui::hud::compass_dial(&mut p, crate::logic::bearing::needle(0.0), 0.0);
        let hint = format!("{}: {}", language.text(Msg::SkyBySun), language.text(Msg::NorthLeft));
        crate::ui::hud::sky_hint(&mut p, &hint);
        let mut vertices = p.into_vertices();
        scale_about(&mut vertices, anchor::TOP(aspect), ui_scale());
        write(&format!("{out}/nav_compass_and_sky{lang}{tag}.png"), &vertices, font);
    }

    // **The first minute**: the one line that sits over the belt for a
    // player who has nothing at all (`hud::first_minute_line`). One
    // picture a language, because the thing to look at is the *length* --
    // the Russian and Polish wordings are half as long again as the
    // English, and a line wider than the belt would be a line running out
    // over the world on a phone.
    //
    // **It was three pictures of three lines.** The other two are on the
    // pack's path tab now and the prompt is one sentence; see
    // `hud::first_minute_line` for why the other two stopped being shown
    // over the belt at all.
    for (language, lang) in [(Language::English, ""), (Language::Russian, "_ru"), (Language::Polish, "_pl")] {
        let mut p = Painter::onto(font, Vec::new());
        crate::ui::hud::first_minute_line(&mut p, language.text(crate::ui::lang::Msg::StepStone));
        let mut vertices = p.into_vertices();
        scale_about(&mut vertices, anchor::BOTTOM(aspect), ui_scale());
        write(&format!("{out}/nav_first_minute{lang}{tag}.png").to_lowercase(), &vertices, font);
    }

    // The chat box asking a cairn's name, lifted over the keyboard the way
    // the frame lifts it.
    {
        let mut chat = crate::ui::chat::Chat::new();
        chat.open_naming((10, 64, 10), "", std::time::Instant::now());
        chat.set_typed_text("Брод у ивы");
        for (language, lang) in [(Language::English, ""), (Language::Russian, "_ru")] {
            let mut vertices = Vec::new();
            chat.build_into(font, aspect, touch, language, std::time::Instant::now(), &mut vertices);
            let grown = layout.fit_from_corner(crate::ui::chat::EXTENT);
            scale_about(&mut vertices, anchor::BOTTOM_LEFT(aspect), grown);
            crate::ui::widgets::lift(&mut vertices, crate::ui::chat::keyboard_lift(touch, true, grown));
            write(&format!("{out}/nav_name_cairn{lang}{tag}.png"), &vertices, font);
        }
    }

    // The map with three cairns on it, two named -- one name long enough
    // to be cut at the frame.
    {
        let mut journal = Journal::new();
        for cx in -6..6 {
            for cz in -4..4 {
                let ground = if (cx + cz) % 3 == 0 { Ground::Forest } else { Ground::Grass };
                journal.explored.insert(ChunkPos::new(cx, cz), Tile::uniform(ground, 64));
            }
        }
        journal.explored.insert(ChunkPos::new(1, 0), Tile::uniform(Ground::Rock, 70).with_cairn(4, 71, 4));
        journal.explored.insert(ChunkPos::new(-2, 1), Tile::uniform(Ground::Grass, 64).with_cairn(8, 65, 8));
        journal.explored.insert(ChunkPos::new(2, -2), Tile::uniform(Ground::Sand, 62).with_cairn(2, 63, 9));
        journal.explored.name_mark((20, 71, 4), "Медь в холмах");
        journal.explored.name_mark((-24, 65, 24), "A spring under the big willow by the ford");
        let player = PlayerMark { x: 0.0, z: 0.0, yaw: -0.4 };
        journal.toggle(Tab::Map);
        let layers = FaceLayers::empty_for_test();
        for (language, lang) in [(Language::English, ""), (Language::Russian, "_ru")] {
            let mut vertices = Vec::new();
            journal.build_into(font, &layers, &a_playing_pack(), player, aspect, language, &mut vertices);
            write(&format!("{out}/nav_map{lang}{tag}.png"), &vertices, font);
        }
    }
}

#[test]
#[ignore = "a tool: writes PNGs of the journal for a person to look at"]
fn journal_snapshot() {
    wear_the_skin();
    use crate::logic::map::{survey, Landmarks};
    use crate::ui::journal::{body_rect, Journal, Tab};
    use crate::ui::map_screen::PlayerMark;
    use primitive_shared::discovery::Discovered;
    use primitive_shared::packed::PackedChunk;
    use primitive_shared::types::{ChunkPos, BLOCK_COPPER_INGOT, BLOCK_NATIVE_COPPER};
    use primitive_shared::worldgen::{Preset, WorldGen};

    let out = std::env::var("UI_SNAPSHOT_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let touch = if crate::ui::widgets::touch_layout() { "_touch" } else { "" };
    let font = FontAtlas::for_size(32, 1_000);
    let layers = FaceLayers::empty_for_test();
    let pack = a_playing_pack();
    let aspect = width() as f32 / height() as f32;

    let mut journal = Journal::new();
    let generator = WorldGen::with_preset(4242, Preset::Normal);
    let started = std::time::Instant::now();
    for cx in -10..10 {
        for cz in -6..6 {
            let pos = ChunkPos::new(cx, cz);
            journal.explored.insert(pos, survey(&PackedChunk::pack(&generator.generate_chunk(pos))));
        }
    }
    println!("[journal] surveyed {} chunks in {:?}", journal.explored.surveyed(), started.elapsed());
    journal.landmarks = Landmarks {
        spawn: Some((8, 70, 8)),
        bags: vec![(-96, 64, 52), (400, 64, -300)],
    };
    let knowledge = Discovered::from_kinds(
        pack.slots()
            .iter()
            .flatten()
            .map(|stack| stack.block)
            .chain([BLOCK_COPPER_INGOT, BLOCK_NATIVE_COPPER, BLOCK_VESSEL, BLOCK_MOULD]),
    );
    journal.set_discovered(knowledge.clone());
    let player = PlayerMark { x: 30.0, z: -12.0, yaw: 0.6 };

    journal.toggle(Tab::Map);
    for (language, tag) in [(Language::English, ""), (Language::Russian, "_ru")] {
        let mut vertices = Vec::new();
        journal.build_into(font, &layers, &pack, player, aspect, language, &mut vertices);
        write(&format!("{out}/journal_map{tag}{touch}.png"), &vertices, font);
    }

    // ...and the same land after a pinch, because a gesture is the one
    // thing a still picture cannot show: two fingers spreading from the
    // middle of the page, fed in exactly as the touch arm feeds them.
    // The pair of files is the before and after of the change that gave
    // the map a pinch at all -- before it, the second finger was dropped
    // and this picture was the one above.
    {
        use crate::platform::TouchPhase;
        let body = body_rect(aspect);
        let middle = (body.centre_x(), body.centre_y());
        let apart = |span: f32| {
            [
                (middle.0 - span, middle.1 - span * 0.5),
                (middle.0 + span, middle.1 + span * 0.5),
            ]
        };
        let (from, to) = (apart(0.15), apart(0.55));
        journal.touch(1, TouchPhase::Started, from[0], aspect, player);
        journal.touch(2, TouchPhase::Started, from[1], aspect, player);
        journal.touch(1, TouchPhase::Moved, to[0], aspect, player);
        journal.touch(2, TouchPhase::Moved, to[1], aspect, player);
        journal.touch(1, TouchPhase::Ended, to[0], aspect, player);
        journal.touch(2, TouchPhase::Ended, to[1], aspect, player);
        let mut vertices = Vec::new();
        journal.build_into(font, &layers, &pack, player, aspect, Language::English, &mut vertices);
        write(&format!("{out}/journal_map_pinched{touch}.png"), &vertices, font);
    }

    // **The ladder page is not here any more**: it is the pack's `ПУТЬ`
    // tab, and it is drawn with the rest of the pack's pages above. See
    // `ui::ladder_screen` for the argument that moved it.

    journal.toggle(Tab::Recipes);
    let row = crate::ui::recipe_book::row_rect(3, body_rect(aspect));
    journal.set_cursor(Some((row.centre_x(), row.centre_y())), aspect, player);
    journal.press(aspect, player);
    for (language, tag) in [(Language::English, ""), (Language::Russian, "_ru")] {
        let mut vertices = Vec::new();
        journal.build_into(font, &layers, &pack, player, aspect, language, &mut vertices);
        write(&format!("{out}/journal_recipes{tag}{touch}.png"), &vertices, font);
    }

    // ...and a row this pack *cannot* make, which is the half of the pane
    // that was added for the player who could not see the ladder: how hot
    // the fire has to be, and everything still missing, by name. A hand row
    // with everything in the pack shows neither.
    {
        let smelting = crate::ui::recipe_book::entries(&knowledge, Default::default(), "")
            .iter()
            .position(|e| primitive_shared::crafting::RECIPES[e.index].station == primitive_shared::crafting::Station::Forge)
            .unwrap_or(0);
        let row = crate::ui::recipe_book::row_rect(smelting, body_rect(aspect));
        journal.set_cursor(Some((row.centre_x(), row.centre_y())), aspect, player);
        journal.press(aspect, player);
        for (language, tag) in [(Language::English, ""), (Language::Russian, "_ru")] {
            let mut vertices = Vec::new();
            journal.build_into(font, &layers, &Inventory::new(), player, aspect, language, &mut vertices);
            write(&format!("{out}/journal_recipes_refused{tag}{touch}.png"), &vertices, font);
        }
    }

    // The give menu, in the three states worth looking at.
    //
    // **The refusal is one of them, and it is the one that had to be
    // looked at.** A test can say the sentence fits; only a picture says
    // whether a player who is not an operator can read, at a glance, that
    // the menu is refusing them rather than broken. In Russian too,
    // because that line is the longest either language has on this page
    // -- and the bar it sits in is the width of the window.
    {
        use crate::ui::give_screen;
        // The page exists only for an operator (`Journal::set_operator`),
        // and this is a picture of the page.
        journal.set_operator(true);
        journal.toggle(Tab::Give);
        let body = body_rect(aspect);
        // Something under the pointer, so the lit cell is in the picture:
        // the second row, which is also where a cell's name has the
        // least room beside its icon.
        let cell = give_screen::cell_rect(give_screen::columns(body) + 1, body);
        journal.set_cursor(Some((cell.centre_x(), cell.centre_y())), aspect, player);
        for (language, tag) in [(Language::English, ""), (Language::Russian, "_ru")] {
            let mut vertices = Vec::new();
            journal.build_into(font, &layers, &pack, player, aspect, language, &mut vertices);
            write(&format!("{out}/journal_give{tag}{touch}.png"), &vertices, font);
        }

        // Asked for, and refused. Driven the way the game drives it --
        // a press on a cell, then the server's own sentence -- rather
        // than by setting the state, so the picture is of a state the
        // game can actually be in.
        journal.press(aspect, player);
        let _ = journal.take_command().expect("a press on a thing asks for it");
        assert!(journal.take_server_note("'give' is operator-only"));
        for (language, tag) in [(Language::English, ""), (Language::Russian, "_ru")] {
            let mut vertices = Vec::new();
            journal.build_into(font, &layers, &pack, player, aspect, language, &mut vertices);
            write(&format!("{out}/journal_give_denied{tag}{touch}.png"), &vertices, font);
        }

        // ...and a search narrowing the grid, which is the other half of
        // what makes two hundred and fifty things usable. Only on a
        // desktop: the field is drawn where there are keys to type into
        // it with (see `give_screen::GiveScreen::paint`), so on a phone
        // this would be the picture above with fewer things in it.
        if !crate::ui::widgets::touch_layout() {
            for c in "copper".chars() {
                journal.type_char(c);
            }
            let mut vertices = Vec::new();
            journal.build_into(font, &layers, &pack, player, aspect, Language::English, &mut vertices);
            write(&format!("{out}/journal_give_search.png"), &vertices, font);
        }
    }

    println!("wrote the journal to {out}");
}

/// Draws the thumb controls over the ground, idle and with the modifier
/// held down.
///
/// **The only way to look at the glass without a phone in your hand.**
/// The arrangement is arithmetic on the window size, and the one thing
/// arithmetic cannot answer is whether the result is somewhere a thumb
/// would go: SHIFT above the stick, the pair of tones on both states,
/// nothing standing on the hotbar.
///
/// ```text
/// UI_SNAPSHOT_SIZE=2712x1220 UI_SNAPSHOT_SCALE=1.5 PRIMITIVE_TOUCH_UI=1 ///   UI_SNAPSHOT_DIR=shots cargo test -p primitive_client --lib ///   thumb_controls_snapshot -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a tool: writes PNGs of the thumb controls for a person to look at"]
fn thumb_controls_snapshot() {
    wear_the_skin();
    use crate::platform::Size;
    use crate::settings::{Emits, TouchLayout};

    let out = std::env::var("UI_SNAPSHOT_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let font = FontAtlas::for_size(32, 1_000);
    let arrangement = TouchLayout::default();
    let layout = crate::platform::touch::Layout::for_size(
        Size::new(width(), height()),
        arrangement,
        ui_scale(),
        false,
    );
    let modifier = (0..TouchLayout::BUTTONS).find(|slot| {
        matches!(
            arrangement.buttons[*slot].emits,
            Emits::Key(crate::platform::Key::ShiftLeft)
        )
    });
    // Both states, because a press *lightens* a frame and a word, and
    // the thing that can go wrong is the state that hides what it is
    // confirming. See the note over `hud::EDGE_PRESSED`.
    for (held, tag) in [(None, "idle"), (modifier, "shift_held")] {
        let mut painter =
            crate::ui::widgets::Painter::onto(font, Vec::new());
        crate::ui::hud::touch_controls(&mut painter, &layout, |slot| Some(slot) == held, Language::English);
        write(
            &format!("{out}/thumb_controls_{tag}.png"),
            &painter.into_vertices(),
            font,
        );
    }
    println!("wrote the thumb controls to {out}");
}

/// Which character each (layer, u, v) is, so text can be drawn back.
fn glyph_lookup(font: FontAtlas) -> std::collections::HashMap<(u32, i32, i32), char> {
    GLYPHS
        .chars()
        .map(|c| {
            let (layer, u, v) = font.place(c);
            ((layer, key(u), key(v)), c)
        })
        .collect()
}

/// UVs come back through a float; a rounded key compares them safely.
fn key(v: f32) -> i32 {
    (v * 10_000.0).round() as i32
}

fn draw_glyph(
    pixels: &mut [[u8; 4]],
    c: char,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    tint: [f32; 4],
) {
    let rows = glyph(c);
    let (w, h) = (x1 - x0, y1 - y0);
    for (row, bits) in rows.iter().enumerate() {
        for column in 0..GLYPH_WIDTH {
            if bits & (1 << (GLYPH_WIDTH - 1 - column)) == 0 {
                continue;
            }
            let px0 = x0 + w * column as f32 / GLYPH_WIDTH as f32;
            let py1 = y1 - h * row as f32 / GLYPH_HEIGHT as f32;
            fill(
                pixels,
                px0,
                py1 - h / GLYPH_HEIGHT as f32,
                px0 + w / GLYPH_WIDTH as f32,
                py1,
                tint,
                1.0,
            );
        }
    }
}

/// Fills one rectangle in UI coordinates, blended by its alpha.
fn fill(pixels: &mut [[u8; 4]], x0: f32, y0: f32, x1: f32, y1: f32, tint: [f32; 4], scale: f32) {
    let aspect = width() as f32 / height() as f32;
    let to_x = |x: f32| ((x / aspect * scale + 1.0) * 0.5 * width() as f32).round() as i32;
    // Y is up in UI coordinates and down in an image.
    let to_y = |y: f32| ((1.0 - (y * scale + 1.0) * 0.5) * height() as f32).round() as i32;
    let (px0, px1) = (to_x(x0).max(0), to_x(x1).min(width() as i32));
    let (py0, py1) = (to_y(y1).max(0), to_y(y0).min(height() as i32));
    let alpha = tint[3].clamp(0.0, 1.0);
    for y in py0..py1 {
        for x in px0..px1 {
            let at = (y as u32 * width() + x as u32) as usize;
            let under = pixels[at];
            for channel in 0..3 {
                let over = tint[channel].clamp(0.0, 1.0) * 255.0;
                pixels[at][channel] =
                    (under[channel] as f32 * (1.0 - alpha) + over * alpha).round() as u8;
            }
        }
    }
}

/// One of the interface's own pictures, decoded once for the process.
///
/// Off the copy compiled into the binary (`embedded::TEXTURES`), which
/// is the same copy the atlas is built from, so a picture redrawn in
/// `assets/textures/ui/` shows up here the next time the crate is built
/// and nowhere else has to be told.
fn skin_picture(piece: crate::ui::widgets::Piece) -> &'static image::RgbaImage {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<&'static str, &'static image::RgbaImage>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("the skin cache");
    *cache.entry(piece.file()).or_insert_with(|| {
        let bytes = crate::embedded::texture(piece.file()).expect("the skin is compiled in");
        let image = image::load_from_memory(bytes).expect("a skin picture").to_rgba8();
        // Leaked rather than kept in the map by value: a picture is a
        // few kilobytes, there are fifteen of them, and the alternative
        // is handing out a reference into a lock that has to be held
        // for as long as the caller draws.
        let leaked: &'static image::RgbaImage = Box::leak(Box::new(image));
        leaked
    })
}

/// Fills a rectangle with a piece of the skin, sampled the way the card
/// samples it.
///
/// **This is the one place this harness stops being a stand-in.** Block
/// icons are drawn as plates here because a block's texture lives on the
/// graphics card; the interface's own pictures do not -- they are in the
/// binary -- so a picture of a screen can show the screen rather than a
/// grey rectangle where the panel goes. Without this, the whole of the
/// skin would be invisible to the one tool built to look at it.
///
/// Nearest, because `texture::build_ui_sampler` is nearest. sRGB out of
/// the byte and back in at the end, because the array is
/// `Rgba8UnormSrgb` and the multiply happens in between; the tint
/// arrives already lifted by `widgets::SKIN_GAIN`.
fn fill_picture(
    pixels: &mut [[u8; 4]],
    picture: &image::RgbaImage,
    (x0, y0, x1, y1): (f32, f32, f32, f32),
    (u0, v0, u1, v1): (f32, f32, f32, f32),
    tint: [f32; 4],
) {
    let aspect = width() as f32 / height() as f32;
    let to_x = |x: f32| ((x / aspect + 1.0) * 0.5 * width() as f32).round() as i32;
    let to_y = |y: f32| ((1.0 - (y + 1.0) * 0.5) * height() as f32).round() as i32;
    let (left, right) = (to_x(x0), to_x(x1));
    let (top, bottom) = (to_y(y1), to_y(y0));
    let (px0, px1) = (left.max(0), right.min(width() as i32));
    let (py0, py1) = (top.max(0), bottom.min(height() as i32));
    if px1 <= px0 || py1 <= py0 {
        return;
    }
    let (w, h) = (picture.width().max(1), picture.height().max(1));
    let (across, down) = (((right - left) as f32).max(1.0), ((bottom - top) as f32).max(1.0));
    let to_linear = |byte: u8| {
        let c = byte as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    for y in py0..py1 {
        // Where in the quad this row falls, and therefore where in the
        // picture. Measured at the pixel's middle, which is what stops
        // the last row of one tile coming out of the next tile.
        let share = (y as f32 + 0.5 - top as f32) / down;
        let sy = (((v0 + (v1 - v0) * share) * h as f32) as i64).clamp(0, h as i64 - 1) as u32;
        for x in px0..px1 {
            let share = (x as f32 + 0.5 - left as f32) / across;
            let sx = (((u0 + (u1 - u0) * share) * w as f32) as i64).clamp(0, w as i64 - 1) as u32;
            let texel = picture.get_pixel(sx, sy).0;
            let alpha = (texel[3] as f32 / 255.0) * tint[3].clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            let at = (y as u32 * width() + x as u32) as usize;
            let under = pixels[at];
            for channel in 0..3 {
                let over = (to_linear(texel[channel]) * tint[channel]).clamp(0.0, 1.0) * 255.0;
                pixels[at][channel] =
                    (under[channel] as f32 * (1.0 - alpha) + over * alpha).round() as u8;
            }
        }
    }
}

fn save_png(path: &str, pixels: &[[u8; 4]]) {
    let mut flat = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        flat.extend_from_slice(pixel);
    }
    let image: image::RgbaImage =
        image::ImageBuffer::from_raw(width(), height(), flat).expect("the buffer is the right size");
    image.save(path).expect("write png");
}

// ---- block models ----
//
// **The blocks that are not boxes.** A drying rack is five poles and a
// skin (see `mesh::rack_block`), and there is no way to judge five poles
// from the numbers: the question is whether it reads as a frame with a
// hide stretched in it from a few paces away, and the only way to answer
// that is to look.
//
// Drawn through the mesher's own emitter, so what is on the picture is
// what the game builds -- the same reason the view model has a tool.

/// Draws the drying rack, loaded and empty, into PNGs.
///
/// ```text
/// BLOCK_MODEL_DIR=shots cargo test -p primitive_client --lib \
///     block_models -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a tool: draws a block model for a person to look at"]
fn block_models() {
    use primitive_shared::types::{faced, rack_with_hide, Facing, BLOCK_DRYING_RACK, BLOCK_LOG};

    let out = std::env::var("BLOCK_MODEL_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");

    let layers = FaceLayers::numbered_for_test();
    // Which file each layer the model uses is a picture of. The model
    // asks for three; anything else it grew would show up as magenta.
    //
    // **Masked to eight bits**, which is all a terrain vertex carries
    // for a layer (see `Vertex::tinted`): the numbered fixture hands out
    // bigger numbers than that, and comparing the untruncated one
    // silently drops every quad wearing it.
    let pole = layers.layer_for_face(BLOCK_LOG, 2);
    let bar = layers.layer_for_face(primitive_shared::types::BLOCK_STRIPPED_LOG, 2);
    let hide = layers.stretched_hide();
    let sheets = [
        (pole & 0xFF, sprite("terrain/log_side.png")),
        (bar & 0xFF, sprite("terrain/log_top.png")),
        (hide & 0xFF, sprite("hide/stretched_hide.png")),
    ];

    for (name, block) in [
        ("rack_loaded", rack_with_hide(faced(BLOCK_DRYING_RACK, Facing::East), true)),
        ("rack_empty", faced(BLOCK_DRYING_RACK, Facing::East)),
    ] {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        crate::engine::mesh::rack_block(
            [0.0, 0.0, 0.0],
            block,
            crate::engine::mesh::RackColumns::Lone,
            &layers,
            0xFF,
            &mut vertices,
            &mut indices,
        );
        draw_block_model(&format!("{out}/{name}.png"), &vertices, &sheets);
    }
    println!("wrote the block models to {out}");
}

/// The meshed rack with every face painted by its face index, for
/// hunting geometry that stands where no face should be.
///
/// ```text
/// BLOCK_MODEL_DIR=shots cargo test -p primitive_client --lib \
///     rack_face_debug -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a tool: paints the meshed rack's faces by index for a person to look at"]
fn rack_face_debug() {
    use crate::logic::chunk_manager::ChunkManager;
    use primitive_shared::lighting::LightMap;
    use primitive_shared::types::{
        faced, Chunk, ChunkPos, Facing, BLOCK_AIR, BLOCK_DRYING_RACK, BLOCK_GRASS,
        CHUNK_SIZE_X, CHUNK_SIZE_Z, CHUNK_VOLUME,
    };

    let out = std::env::var("BLOCK_MODEL_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");

    let pos = ChunkPos::new(0, 0);
    let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
    for z in 0..CHUNK_SIZE_Z {
        for x in 0..CHUNK_SIZE_X {
            blocks[Chunk::index(x, 0, z)] = BLOCK_GRASS;
        }
    }
    // **Loaded**, because the skin is geometry too and the artefact
    // being hunted showed on a loaded frame.
    blocks[Chunk::index(4, 1, 8)] = primitive_shared::types::rack_with_hide(
        faced(BLOCK_DRYING_RACK, Facing::South),
        true,
    );
    let mut chunks = ChunkManager::new(4);
    chunks.insert(Chunk { pos, blocks });
    let mut light = LightMap::new();
    light.load_chunk(&chunks, pos);
    let mut cache = crate::engine::mesh::Neighbourhood::default();
    cache.fill(pos, &chunks, &light);
    let mut mesh = crate::engine::mesh::MeshBuffers::default();
    crate::engine::mesh::build_mesh(
        pos,
        &cache,
        &FaceLayers::empty_for_test(),
        &primitive_shared::worldgen::WorldGen::new(0),
        &mut mesh,
    );

    // The rack's own quads, moved to the origin, wearing their face
    // index as their layer -- so the flat sheets below become the key.
    let mut vertices = Vec::new();
    for quad in mesh.vertices.chunks_exact(4) {
        // Everything standing in the rack's cell, its floor included --
        // an earlier cut of this filter demanded every corner be
        // *above* the floor, which quietly threw away the uprights'
        // side faces, the very quads a blade would hide in.
        let near = quad.iter().all(|v| {
            (3.5..=5.5).contains(&v.position[0])
                && (1.0 - 1e-3..=2.0 + 1e-3).contains(&v.position[1])
                && (7.5..=9.5).contains(&v.position[2])
        });
        // ...but not the ground it stands on: a grass face spans the
        // whole cell, and nothing in this model does.
        let full_cell = quad
            .iter()
            .map(|v| v.position[0])
            .fold(f32::MIN, f32::max)
            - quad.iter().map(|v| v.position[0]).fold(f32::MAX, f32::min)
            > 0.99;
        if !near || full_cell {
            continue;
        }
        for v in quad {
            let face = (v.light() >> 10) & 7;
            vertices.push(crate::engine::mesh::Vertex::new(
                [v.position[0] - 4.0, v.position[1] - 1.0, v.position[2] - 8.0],
                v.uv(),
                face,
                v.light(),
            ));
        }
    }

    let key = [
        [220u8, 40, 40],   // 0 +Y red
        [40, 60, 220],     // 1 -Y blue
        [40, 200, 60],     // 2 +X green
        [230, 220, 40],    // 3 -X yellow
        [40, 210, 210],    // 4 +Z cyan
        [220, 60, 220],    // 5 -Z magenta
    ];
    let sheets: Vec<(u32, image::RgbaImage)> = key
        .iter()
        .enumerate()
        .map(|(face, colour)| {
            let mut img = image::RgbaImage::new(2, 2);
            for p in img.pixels_mut() {
                *p = image::Rgba([colour[0], colour[1], colour[2], 255]);
            }
            (face as u32, img)
        })
        .collect();
    // The numbers, because a picture of a thin blade cannot say which
    // box grew it. One line per quad: which way it faces, and the box
    // it spans in sixteenths of the cell.
    const FACE_NAMES: [&str; 6] = ["+Y top", "-Y bottom", "+X east", "-X west", "+Z south", "-Z north"];
    println!("{} quads:", vertices.len() / 4);
    for quad in vertices.chunks_exact(4) {
        let span = |axis: usize| {
            let lo = quad.iter().map(|v| v.position[axis]).fold(f32::MAX, f32::min) * 16.0;
            let hi = quad.iter().map(|v| v.position[axis]).fold(f32::MIN, f32::max) * 16.0;
            (lo, hi)
        };
        let (x0, x1) = span(0);
        let (y0, y1) = span(1);
        let (z0, z1) = span(2);
        let face = quad[0].tex_layer() as usize;
        println!(
            "  {:<10} x {:5.1}..{:5.1}  y {:5.1}..{:5.1}  z {:5.1}..{:5.1}",
            FACE_NAMES.get(face).copied().unwrap_or("?"),
            x0, x1, y0, y1, z0, z1
        );
    }

    // Two seats. The three-quarter one is how a model is judged; the
    // close, wide-angle one is where the blades were photographed, and
    // an artefact that only shows from one seat is exactly the kind
    // this tool exists to name.
    draw_block_model_from(
        &format!("{out}/rack_faces.png"),
        &vertices,
        &sheets,
        glam::Vec3::new(2.3, 1.9, 2.6),
        glam::Vec3::new(0.5, 0.55, 0.5),
        40.0,
    );
    draw_block_model_from(
        &format!("{out}/rack_faces_near.png"),
        &vertices,
        &sheets,
        glam::Vec3::new(0.5, 0.7, 1.35),
        glam::Vec3::new(0.5, 0.55, 0.5),
        95.0,
    );
    println!("wrote the face-debug model to {out}");
}

/// One of the game's own textures, decoded.
fn sprite(name: &str) -> image::RgbaImage {
    let bytes = crate::embedded::texture(name).expect("a texture this build ships");
    image::load_from_memory(bytes).expect("a PNG").to_rgba8()
}

/// Rasterises a model's quads from a three-quarter view.
///
/// Painter's algorithm on the far corner of each quad, which for a
/// handful of boxes standing in one cell is the same answer a depth
/// buffer would give and a tenth of the code.
fn draw_block_model(
    path: &str,
    vertices: &[crate::engine::mesh::Vertex],
    sheets: &[(u32, image::RgbaImage)],
) {
    // Looking down at the cell from off one corner, which is how the
    // reference for this model was drawn and roughly how a player meets
    // one.
    draw_block_model_from(
        path,
        vertices,
        sheets,
        glam::Vec3::new(2.3, 1.9, 2.6),
        glam::Vec3::new(0.5, 0.55, 0.5),
        40.0,
    );
}

/// The same, from a seat the caller picks.
///
/// **A model has to be judged from more than one chair.** Three
/// separate artefacts in this file's history were invisible from the
/// three-quarter view above and obvious from a pace away with a wide
/// lens, which is where a player actually stands.
fn draw_block_model_from(
    path: &str,
    vertices: &[crate::engine::mesh::Vertex],
    sheets: &[(u32, image::RgbaImage)],
    eye: glam::Vec3,
    at: glam::Vec3,
    fov_degrees: f32,
) {
    let mut pixels = vec![[58u8, 62, 70, 255]; (width() * height()) as usize];

    let view = glam::Mat4::look_at_rh(eye, at, glam::Vec3::Y);
    let projection = glam::Mat4::perspective_rh(
        fov_degrees * std::f32::consts::PI / 180.0,
        width() as f32 / height() as f32,
        0.05,
        20.0,
    );
    let clip = projection * view;

    let mut quads: Vec<&[crate::engine::mesh::Vertex]> = vertices.chunks_exact(4).collect();
    quads.sort_by(|a, b| {
        let depth = |quad: &&[crate::engine::mesh::Vertex]| {
            quad.iter()
                .map(|v| (glam::Vec3::from_array(v.position) - eye).length())
                .fold(0.0f32, f32::max)
        };
        depth(b).partial_cmp(&depth(a)).unwrap_or(std::cmp::Ordering::Equal)
    });

    for quad in quads {
        let Some((_, sheet)) = sheets.iter().find(|(layer, _)| *layer == quad[0].tex_layer())
        else {
            continue;
        };
        let screen: Vec<(f32, f32)> = quad
            .iter()
            .map(|v| {
                let point = clip * glam::Vec3::from_array(v.position).extend(1.0);
                let ndc = point.truncate() / point.w.max(1e-6);
                (
                    (ndc.x * 0.5 + 0.5) * width() as f32,
                    (1.0 - (ndc.y * 0.5 + 0.5)) * height() as f32,
                )
            })
            .collect();
        // The light word the mesher packed, read back as the shade the
        // shader would give this face -- so a picture of a pole has the
        // same four sides the game draws.
        let face = (quad[0].packed >> 10) & 0b111;
        let lit = match face {
            0 => 1.0,
            1 => 0.55,
            2 | 3 => 0.80,
            _ => 0.68,
        };
        // In pictures, as the shader reads them: `uv()` decodes a fine
        // coordinate the way `terrain_vertex` does, so a cut model face
        // is drawn here as the game draws it -- see `mesh::FINE_UV_BIT`.
        let uvs: Vec<(f32, f32)> = quad.iter().map(|v| (v.uv()[0], v.uv()[1])).collect();
        for triangle in [[0usize, 1, 2], [0, 2, 3]] {
            fill_triangle(
                &mut pixels,
                [screen[triangle[0]], screen[triangle[1]], screen[triangle[2]]],
                [uvs[triangle[0]], uvs[triangle[1]], uvs[triangle[2]]],
                sheet,
                lit,
            );
        }
    }
    save_png(path, &pixels);
}

// ---- the view model ----
//
// **The one thing on screen that is not a flat panel.** The grip a tool
// is held in is four angles -- a centre, a scale, a yaw and a roll --
// that mean nothing apart from each other, and the only honest way to
// tune one is to look at the result. Doing that in the game means
// building, launching, finding a tree and taking a photograph; doing it
// here means running one test.
//
// It draws the same transform the game does (`hand::item_transform`)
// through the same projection (`HAND_FOV_Y`, 70 degrees), so a pose that
// reads here reads in the frame.

/// Draws whatever is in the hand into a PNG, from the eye's own seat.
///
/// ```text
/// HAND_POSE_DIR=shots cargo test -p primitive_client --lib \
///     hand_pose -- --ignored --nocapture
/// ```
///
/// `--lib`, not `--bin primitive_client`: every runbook line in this
/// crate said the binary, `main.rs` carries no tests, and the filter
/// therefore matched nothing and printed "running 0 tests" -- a tool
/// that looks like it ran and writes no picture.
#[test]
#[ignore = "a tool: draws the grip a tool is held in for a person to look at"]
fn hand_pose() {
    let out = std::env::var("HAND_POSE_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    // Tools *and* materials, because they are held differently and the
    // difference is the whole point of `hand::held_scale`. Holding a
    // lump of native copper in the grip of an axe is what this tool
    // would have caught: it covered the bottom-right quarter of the
    // frame, and nothing here drew anything but a tool.
    for (texture, block) in [
        ("tools/stone_axe.png", "stone_axe"),
        ("tools/stone_pickaxe.png", "stone_pickaxe"),
        ("tools/flint_knife.png", "flint_knife"),
        ("metal/native_copper.png", "native_copper"),
        ("tools/flint.png", "flint"),
        ("plants/stick.png", "stick"),
        ("plants/fiber.png", "fiber"),
        // The torch, alight: the one held thing with something drawn
        // *over* it. The flame is a separate quad and its own light, so
        // a grip that looked right for an axe can still be wrong here.
        ("tools/torch_lit.png", "torch_lit"),
        ("tools/torch.png", "torch"),
        // The spear, which is the one thing here held by its *ends*
        // rather than by a pair of angles -- pointing down the line of
        // sight, so what this draws is mostly foreshortening. See
        // `hand::spear_transform`; a painter with no depth buffer is
        // the wrong tool for judging a plate seen at an angle, so this
        // is a sanity check and the photograph in the game is the
        // proof.
        ("tools/flint_spear.png", "flint_spear"),
    ] {
        let name = texture.rsplit('/').next().unwrap_or(texture);
        draw_held(&format!("{out}/hand_{name}"), texture, block);
    }
    println!("wrote the view model to {out}");
}

/// One thing, in the hand, at rest.
fn draw_held(path: &str, texture: &str, block_name: &str) {
    use crate::engine::item_model::{ItemModel, ItemVertex};

    let bytes = crate::embedded::texture(texture).expect("a picture this build ships");
    let sprite = image::load_from_memory(bytes).expect("a PNG").to_rgba8();
    let model = ItemModel::from_image(&sprite);
    let block = primitive_shared::types::ALL_BLOCK_IDS
        .iter()
        .find(|(_, name)| *name == block_name)
        .map(|&(id, _)| id)
        .expect("a block this build knows");

    let mut vertices: Vec<ItemVertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    model.append_transformed(
        &mut vertices,
        &mut indices,
        // The whole transform the game uses, slide and all -- see
        // `hand::held_transform`. Taking only the grip would draw a
        // torch a fifth of its own length away from where the player
        // sees it, which is the one thing this tool exists not to do.
        crate::logic::hand::held_transform(glam::Mat4::IDENTITY, block, &model),
        0,
        15,
        0,
    );

    // The world behind the hand, so the pose is judged against
    // something: a horizon and the crosshair the tool is meant to be
    // pointing past.
    let mut pixels = vec![[104u8, 138, 176, 255]; (width() * height()) as usize];
    for y in height() / 2..height() {
        for x in 0..width() {
            pixels[(y * width() + x) as usize] = [92, 116, 78, 255];
        }
    }

    let projection = glam::Mat4::perspective_rh(
        70.0 * std::f32::consts::PI / 180.0,
        width() as f32 / height() as f32,
        0.01,
        4.0,
    );

    // Painter's algorithm, back to front. A depth buffer would be more
    // correct and this is a picture of forty quads: the far plate is
    // drawn, then the rim, then the near plate, and the order is what
    // the eye would see anyway.
    let mut quads: Vec<[ItemVertex; 4]> = vertices
        .chunks_exact(4)
        .map(|quad| [quad[0], quad[1], quad[2], quad[3]])
        .collect();
    quads.sort_by(|a, b| {
        let depth = |quad: &[ItemVertex; 4]| {
            quad.iter().map(|v| v.position[2]).sum::<f32>() / 4.0
        };
        depth(a).partial_cmp(&depth(b)).unwrap_or(std::cmp::Ordering::Equal)
    });

    for quad in &quads {
        let corners: Vec<glam::Vec3> = quad
            .iter()
            .map(|v| glam::Vec3::from_array(v.position))
            .collect();
        // Flat shading off the winding, exactly the way the hand's own
        // vertices take their light -- see `hand::push_quad`.
        let normal = (corners[1] - corners[0])
            .cross(corners[2] - corners[1])
            .normalize_or_zero();
        let lit = 0.55 + 0.45 * normal.dot(glam::Vec3::new(-0.3, 0.8, 0.5).normalize()).max(0.0);
        let screen: Vec<(f32, f32)> = corners
            .iter()
            .map(|&point| {
                let clip = projection * point.extend(1.0);
                let ndc = clip.truncate() / clip.w.max(1e-6);
                (
                    (ndc.x * 0.5 + 0.5) * width() as f32,
                    (1.0 - (ndc.y * 0.5 + 0.5)) * height() as f32,
                )
            })
            .collect();
        let uvs: Vec<(f32, f32)> = quad.iter().map(|v| (v.uv[0], v.uv[1])).collect();
        for triangle in [[0usize, 1, 2], [0, 2, 3]] {
            fill_triangle(
                &mut pixels,
                [screen[triangle[0]], screen[triangle[1]], screen[triangle[2]]],
                [uvs[triangle[0]], uvs[triangle[1]], uvs[triangle[2]]],
                &sprite,
                lit,
            );
        }
    }

    // The crosshair, last and over everything: what the tool is supposed
    // to be aimed past.
    for offset in -10i32..=10 {
        for (x, y) in [
            (width() as i32 / 2 + offset, height() as i32 / 2),
            (width() as i32 / 2, height() as i32 / 2 + offset),
        ] {
            pixels[(y as u32 * width() + x as u32) as usize] = [250, 250, 250, 255];
        }
    }
    save_png(path, &pixels);
}

/// One textured triangle, nearest-sampled, with alpha as a cutout.
fn fill_triangle(
    pixels: &mut [[u8; 4]],
    points: [(f32, f32); 3],
    uvs: [(f32, f32); 3],
    sprite: &image::RgbaImage,
    lit: f32,
) {
    let min_x = points.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0) as u32;
    let max_x = (points.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as i64)
        .clamp(0, width() as i64) as u32;
    let min_y = points.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as u32;
    let max_y = (points.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i64)
        .clamp(0, height() as i64) as u32;
    let area = edge(points[0], points[1], points[2]);
    if area.abs() < 1e-6 {
        return;
    }
    for y in min_y..max_y {
        for x in min_x..max_x {
            let at = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(points[1], points[2], at) / area;
            let w1 = edge(points[2], points[0], at) / area;
            let w2 = edge(points[0], points[1], at) / area;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let u = w0 * uvs[0].0 + w1 * uvs[1].0 + w2 * uvs[2].0;
            let v = w0 * uvs[0].1 + w1 * uvs[1].1 + w2 * uvs[2].1;
            // Wrapped rather than clamped: a block model maps its
            // texture by *size* and a pole two metres long asks for the
            // picture twice, exactly as the game's sampler repeats it.
            let sx = ((u * sprite.width() as f32) as i32).rem_euclid(sprite.width() as i32) as u32;
            let sy = ((v * sprite.height() as f32) as i32).rem_euclid(sprite.height() as i32) as u32;
            let texel = sprite.get_pixel(sx, sy).0;
            if texel[3] < 128 {
                continue;
            }
            let at = (y * width() + x) as usize;
            for channel in 0..3 {
                pixels[at][channel] = (texel[channel] as f32 * lit).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

fn edge(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// The barter stall, as its owner and as a buyer see it, in English and
/// Russian: the owner halfway through a second price, the buyer's pointer on
/// TRADE.
///
/// ```text
/// UI_SNAPSHOT_DIR=shots/stall cargo test -p primitive_client --lib \
///     stall_snapshot -- --ignored --nocapture
/// UI_SNAPSHOT_SIZE=2712x1220 UI_SNAPSHOT_SCALE=1.65 PRIMITIVE_TOUCH_UI=1 \
///     UI_SNAPSHOT_DIR=shots/stall/phone cargo test -p primitive_client --lib \
///     stall_snapshot -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a tool: writes PNGs of the stall for a person to look at"]
fn stall_snapshot() {
    wear_the_skin();
    use crate::ui::chest_screen::{stall_control_rect, StallControl, StallView};
    use primitive_shared::stall::{Offer, STOCK, TAKINGS};
    use primitive_shared::types::{BLOCK_HIDE, BLOCK_STALL};

    let out = std::env::var("UI_SNAPSHOT_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let font = FontAtlas::for_size(32, 1_000);
    let layers = FaceLayers::empty_for_test();
    let mut pack = a_playing_pack();
    pack.add(BLOCK_HIDE, 3);
    let at = (0, 0, 0);
    let offers = vec![
        Some(Offer { give: BLOCK_FLINT, give_count: 4, take: BLOCK_HIDE, take_count: 1 }),
        Some(Offer { give: BLOCK_STONE_AXE, give_count: 1, take: BLOCK_RAW_MEAT, take_count: 6 }),
        None,
    ];
    let mut store = Inventory::chest();
    store.add_within(STOCK, BLOCK_FLINT, 22);
    store.add_within(STOCK, BLOCK_STONE_AXE, 1);
    store.add_within(TAKINGS, BLOCK_HIDE, 2);
    for (yours, name) in [(true, "owner"), (false, "buyer")] {
        for (language, tag) in [(Language::English, "en"), (Language::Russian, "ru")] {
            let mut screen = ChestScreen::new();
            screen.show_stall(StallView { at, owner: "Ada".to_string(), yours, offers: offers.clone() });
            screen.show(at, store.clone(), Some(BLOCK_STALL), ContainerKind::Stall, None, None);
            let point = stall_control_rect(StallControl::Action(0));
            screen.set_cursor(Some((point.centre_x(), point.centre_y())));
            write_grown(
                &format!("{out}/stall_{name}_{tag}.png"),
                &screen.build(font, &layers, &pack, language),
                font,
                screen.grow_by(snapshot_layout()),
            );
        }
    }
}

/// Every screen a player can open, in English and in Russian, at whatever
/// size `UI_SNAPSHOT_SIZE` says -- the audit's before-and-after.
///
/// **One test for all of them, because the argument is between them.** The
/// tools above were each written beside one screen by whoever was building
/// it, and each answers "does this screen look right"; none of them answers
/// "do these screens look like one game", which is the question a folder of
/// them side by side is for. Run once a size:
///
/// ```text
/// UI_SNAPSHOT_SIZE=2712x1220 UI_SNAPSHOT_SCALE=1.5 PRIMITIVE_TOUCH_UI=1 \
///   UI_SNAPSHOT_DIR=shots/ui_audit/after/phone cargo test -p primitive_client \
///   --lib ui_audit_snapshot -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a tool: writes PNGs of every screen for a person to look at"]
fn ui_audit_snapshot() {
    wear_the_skin();
    use crate::ui::inventory_screen::Vitals;
    use crate::ui::station_screen::{jobs_of, StationScreen};
    use primitive_shared::minigame::{tolerance, Game};
    use primitive_shared::quality::Quality;
    use primitive_shared::tools::with_edge;
    use primitive_shared::types::{BLOCK_BRONZE_AXE, BLOCK_COOKED_MEAT, BLOCK_COPPER_SAW};

    let out = std::env::var("UI_SNAPSHOT_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let font = FontAtlas::for_size(32, 1_000);
    let layers = FaceLayers::empty_for_test();
    let aspect = width() as f32 / height() as f32;
    let touch = crate::ui::widgets::touch_layout();

    // A pack whose belt carries every mark a slot can wear, side by side:
    // fine and poor, dull and blunt, worn, a jug with grain in it, and one
    // tool that is all of dull, worn and fine at once -- the slot where the
    // corners would fight if they were going to.
    let mut pack = a_playing_pack();
    pack.open_backpack();
    for (slot, stack) in [
        (0, Stack::new(BLOCK_COOKED_MEAT, 5).with_quality(Quality::from_fraction(0.97))),
        (1, Stack::new(BLOCK_COOKED_MEAT, 2).with_quality(Quality::from_fraction(0.05))),
        (2, Stack::new(with_edge(BLOCK_COPPER_SAW, 2), 1)),
        (3, Stack::new(with_edge(BLOCK_BRONZE_AXE, 3), 1)),
        (4, Stack::worn(with_edge(BLOCK_BRONZE_AXE, 2), 1, 400).with_quality(Quality::from_fraction(0.97))),
        (5, primitive_shared::inventory::filled_jug(primitive_shared::types::BLOCK_SEEDS, 11)),
    ] {
        pack.take_slot(slot);
        pack.put_in_slot(slot, stack);
    }
    for square in [0, 3, 7] {
        pack.put_in_slot(primitive_shared::inventory::SLOTS + square, Stack::new(BLOCK_CLAY, 9 + square as u32));
    }
    let pack_scale = crate::ui::inventory_screen::grow_by(snapshot_layout());
    let poorly = Vitals {
        health: 0.35,
        nourishment: 0.2,
        stamina: 0.5,
        body: crate::ui::hud::BodyGauges {
            temperature_c: 33.0,
            comfort: primitive_shared::body::Comfort::of(33.0),
            hydration: 0.15,
            fatigue: 0.8,
            injuries: snapshot_wounds(),
            wetness: 0.6,
            grime: 0.5,
            recovery: 0.6,
            diet_groups: 1,
            ..crate::ui::hud::BodyGauges::default()
        },
    };

    for (language, lang) in [(Language::English, "en"), (Language::Russian, "ru")] {
        let shot = |name: &str, vertices: &[HotbarVertex], grown: f32| {
            write_grown(&format!("{out}/{name}_{lang}.png"), vertices, font, grown);
        };

        // The pack's three pages, and a slot tooltip over the marked belt.
        for (tab, name) in [
            (crate::ui::inventory_screen::Tab::Health, "inv_health"),
            (crate::ui::inventory_screen::Tab::Pack, "inv_pack"),
            (crate::ui::inventory_screen::Tab::Backpack, "inv_rucksack"),
            (crate::ui::inventory_screen::Tab::Learn, "inv_path"),
        ] {
            let mut screen = InventoryScreen::new();
            screen.open = true;
            screen.sync(&pack);
            screen.set_tab(tab);
            let hover = crate::ui::inventory_screen::slot_rect(4);
            screen.set_cursor(Some((hover.centre_x(), hover.centre_y())));
            let mut v = Vec::new();
            // Part way up the ladder, so the path page has lit rungs,
            // dim ones and a rung wanting something never held.
            let seen = primitive_shared::discovery::Discovered::from_kinds([
                primitive_shared::types::BLOCK_PEBBLE,
                primitive_shared::types::BLOCK_FLINT_FLAKE,
                primitive_shared::types::BLOCK_CAMPFIRE,
                primitive_shared::types::BLOCK_KILN,
                primitive_shared::types::BLOCK_COPPER_INGOT,
            ]);
            let keys = crate::ui::keybinds::Keybinds::default();
            screen.build_into(font, &layers, &pack, &primitive_shared::inventory::Equipment::new(), &snapshot_wounds(), &poorly, crate::ui::ladder_screen::Learning { discovered: &seen, keys: &keys }, language, &mut v);
            shot(name, &v, pack_scale);
        }
        {
            let mut screen = InventoryScreen::new();
            screen.open = true;
            screen.sync(&pack);
            let row = crate::ui::inventory_screen::recipe_rect(0, 0);
            screen.set_cursor(Some((row.centre_x(), row.centre_y())));
            shot("inv_recipe_tip", &screen.build(font, &layers, &pack, 0.7, language), pack_scale);
        }

        // Containers: chest, body with its rucksack page, bags, hearth, rack, stall.
        let container = |kind: ContainerKind, contents: Inventory, block, name: &str, hover: Option<(f32, f32)>| {
            let mut screen = ChestScreen::new();
            screen.show((0, 0, 0), contents, Some(block), kind, None, None);
            screen.set_cursor(hover);
            shot(name, &screen.build(font, &layers, &pack, language), screen.grow_by(snapshot_layout()));
        };
        let chest_hover = crate::ui::chest_screen::slot_rect(primitive_shared::protocol::Side::Chest, 1);
        container(ContainerKind::Chest, a_full_chest(), primitive_shared::types::BLOCK_CHEST, "chest", Some((chest_hover.centre_x(), chest_hover.centre_y())));
        container(ContainerKind::Saddlebags, a_full_chest(), primitive_shared::types::BLOCK_SADDLEBAGS, "bags", None);
        {
            let mut contents = Inventory::body(true);
            for offset in [0, 3, 11] {
                contents.put_in_slot(primitive_shared::inventory::CORPSE_COMPARTMENT.start + offset, Stack::new(BLOCK_FLINT, 3));
            }
            let mut body = ChestScreen::new();
            body.show((0, 0, 0), contents, Some(primitive_shared::types::BLOCK_CORPSE), ContainerKind::Chest, None, None);
            let tab = crate::ui::chest_screen::page_tab_rect(true);
            body.set_cursor(Some((tab.centre_x(), tab.centre_y())));
            let _ = body.click(&pack, crate::ui::inventory_screen::Button::Left, false, false);
            shot("corpse_rucksack", &body.build(font, &layers, &pack, language), body.grow_by(snapshot_layout()));
        }
        {
            let mut kiln = ChestScreen::new();
            kiln.show(
                (0, 0, 0),
                a_working_kiln(),
                Some(primitive_shared::types::BLOCK_KILN_LIT),
                ContainerKind::Hearth(hearth::Kind::Kiln),
                Some(HearthState { fuel_left: 74.0, progress: 0.42, degrees: 1180.0, needs: hearth::COPPER_MELTS_C, wet: false }),
                None,
            );
            shot("hearth", &kiln.build(font, &layers, &pack, language), kiln.grow_by(snapshot_layout()));
            let mut rack = ChestScreen::new();
            rack.show(
                (0, 0, 0),
                a_loaded_rack(),
                Some(primitive_shared::types::BLOCK_DRYING_RACK),
                ContainerKind::Rack,
                None,
                Some(RackState { progress: 0.38, rate: 0.0, wet: true, near_fire: false }),
            );
            shot("rack", &rack.build(font, &layers, &pack, language), rack.grow_by(snapshot_layout()));
        }
        {
            use crate::ui::chest_screen::StallView;
            use primitive_shared::stall::{Offer, STOCK};
            let mut store = Inventory::chest();
            store.add_within(STOCK, BLOCK_FLINT, 22);
            let mut stall = ChestScreen::new();
            stall.show_stall(StallView {
                at: (0, 0, 0),
                owner: "Ada".to_string(),
                yours: false,
                offers: vec![Some(Offer { give: BLOCK_FLINT, give_count: 4, take: primitive_shared::types::BLOCK_HIDE, take_count: 1 }), None, None],
            });
            stall.show((0, 0, 0), store, Some(primitive_shared::types::BLOCK_STALL), ContainerKind::Stall, None, None);
            shot("stall_buyer", &stall.build(font, &layers, &pack, language), stall.grow_by(snapshot_layout()));
        }

        // The four stations, at rest and mid-run.
        for (game, name) in [(Game::Anvil, "anvil"), (Game::Wheel, "wheel"), (Game::Whet, "whet"), (Game::Saw, "saw")] {
            let mut list = StationScreen::new();
            list.asked_to_open();
            list.show(game, tolerance(None));
            let row = crate::ui::station_screen::Panel::for_game(game).row(0);
            list.set_cursor(Some((row.centre_x(), row.centre_y())));
            let mut v = Vec::new();
            list.build_into(font, &layers, language, &mut v);
            shot(&format!("{name}_list"), &v, list.grow_by(snapshot_layout()));
            if let Some(&job) = jobs_of(game).first() {
                let run = StationScreen::mid_run(game, tolerance(None), job, 0x5EED, 900);
                let mut v = Vec::new();
                run.build_into(font, &layers, language, &mut v);
                shot(&format!("{name}_run"), &v, run.grow_by(snapshot_layout()));
            }
        }

        // The death notice.
        {
            let mut death = crate::ui::death::DeathScreen::new();
            death.open(crate::ui::names::death_cause("fell from a great height", language).into_owned());
            for _ in 0..40 {
                death.tick(0.05);
            }
            shot("death", &death.build(font, language), crate::ui::death::grow_by(snapshot_layout()));
        }

        // The whole HUD at once: every gauge in a bad state, a refusal, the
        // line, the sail with the compass beside it, the sky's word and --
        // on a phone -- the thumbs. Nobody plays with all of it up, and that
        // is the point: anything that can collide does so here.
        {
            use crate::ui::hotbar::{BOTTOM, PAD, SLOT};
            use crate::ui::widgets::{anchor, scale_about, Painter};
            let mut v = Vec::new();
            let box_quad = |v: &mut Vec<HotbarVertex>, x0: f32, y0: f32, x1: f32, y1: f32, tint: [f32; 4]| {
                for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y0), (x1, y1), (x0, y1)] {
                    v.push(HotbarVertex { position: [x, y], uv: [0.0, 0.0], tex_layer: UNTEXTURED, tint });
                }
            };
            let total = (SLOT + 0.012) * 10.0 - 0.012;
            box_quad(&mut v, -total / 2.0 - PAD, BOTTOM - PAD, total / 2.0 + PAD, BOTTOM + SLOT + PAD, [0.12, 0.12, 0.14, 0.92]);
            for slot in 0..10 {
                let centre = crate::ui::hotbar::slot_centre(slot, 10);
                box_quad(&mut v, centre - SLOT / 2.0, BOTTOM, centre + SLOT / 2.0, BOTTOM + SLOT, [0.30, 0.31, 0.34, 1.0]);
            }
            let refusal = match language {
                Language::Russian => "возвращён: слишком далеко от мира",
                _ => "moved back: too far from the world",
            };
            v.extend(crate::ui::hud::build(font, 7.0, 20.0, 12.0, 0.2, false, 0.4, 0.15, poorly.body, &pack, Some((refusal, 1.0))));
            scale_about(&mut v, anchor::BOTTOM(aspect), ui_scale());

            let mut line = Painter::onto(font, Vec::new());
            crate::ui::hud::line_gauge(&mut line, Some(0.6), Some(0.8));
            let mut line = line.into_vertices();
            scale_about(&mut line, anchor::CENTRE(aspect), ui_scale());
            v.extend(line);

            let mut top = Painter::onto(font, Vec::new());
            crate::ui::hud::sail_gauge(&mut top, 0.7, 0.3, 0.8, false);
            crate::ui::hud::compass_dial(&mut top, crate::logic::bearing::needle(0.4), crate::ui::hud::COMPASS_BESIDE_SAIL);
            let hint = format!("{}: {}", language.text(crate::ui::lang::Msg::SkyByStars), language.text(crate::ui::lang::Msg::NorthBehind));
            crate::ui::hud::sky_hint(&mut top, &hint);
            let mut top = top.into_vertices();
            scale_about(&mut top, anchor::TOP(aspect), ui_scale());
            v.extend(top);

            if touch {
                let mut thumbs = Painter::onto(font, Vec::new());
                let controls = crate::platform::touch::Layout::for_size(
                    crate::platform::Size::new(width(), height()),
                    crate::settings::TouchLayout::default(),
                    ui_scale(),
                    false,
                );
                crate::ui::hud::touch_controls(&mut thumbs, &controls, |_| false, language);
                v.extend(thumbs.into_vertices());
            }
            write(&format!("{out}/hud_{lang}.png"), &v, font);
        }
    }
    println!("wrote the audit to {out}");
}
