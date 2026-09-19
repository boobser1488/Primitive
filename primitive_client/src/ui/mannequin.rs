//! The body in the pack: a figure of flat shapes that shows where it hurts
//! and takes a dressing dropped on the part that needs it.
//!
//! ## Why a figure and not a list
//!
//! A list of wounds -- "left arm: cut, bleeding" -- answers the question
//! only once it has been read, and the question a hurt player is asking
//! is *where*. A figure answers it before anything is read: the arm is
//! red. It is also the drop target, which is the other half of why it is
//! a picture: a bandage is picked up out of the pack and put *on the arm*,
//! and there is no gesture a list could offer that is as obvious.
//!
//! ## Why flat shapes
//!
//! Untextured on purpose. The figure is a diagram of a body, not a
//! portrait of the player's -- it has no clothes, no skin tone and no face
//! -- because the only information it carries is colour and a mark per
//! wound, and a textured body would put detail exactly where the colour
//! has to be read. It also costs no layer of the texture array, which is
//! the budget everything else in this game is fighting over.
//!
//! ## Which side is left
//!
//! **The left arm is on the left of the screen**, as a player looking
//! down at their own body sees it, and not mirrored the way a person
//! facing you would be. A mirror would put "left arm" under the right
//! thumb, and the tooltip would spend its first word correcting the
//! picture. The tooltip names the part in words anyway, so neither reading
//! can be mistaken for the other for long.
//!
//! ## The rule this file shares with every screen
//!
//! **What is hit-tested is exactly what is drawn**: `part_rect` is the one
//! source of both, and `part_at` is its inverse. A test says so, and the
//! screen that holds the figure has a second one that goes through the
//! interface scale as well.

use primitive_shared::injury::{heals_alone, Injuries, Kind, Part, Treatment, Wound};
use primitive_shared::types::BlockId;

use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{Painter, Rect};

/// Where each part sits inside the figure's box, as fractions of it:
/// left, bottom, right, top, with y going up.
///
/// **Gaps between every pair of parts**, which is not styling: a point on
/// a seam must belong to one part or to none, and two rectangles that
/// touch would have the arm and the body both answering it -- decided by
/// the order they were asked in, which is a rule nobody can see. The gaps
/// are also what makes six rectangles read as a body rather than as a
/// block with lines on it.
///
/// Proportions from the eight-heads figure every drawing class starts
/// from, squashed to the box: a head a fifth of the height, a body a third,
/// legs the rest; arms as long as the body and a little more.
const LAYOUT: [(Part, [f32; 4]); 6] = [
    (Part::Head, [0.36, 0.80, 0.64, 1.00]),
    (Part::Torso, [0.29, 0.47, 0.71, 0.77]),
    (Part::LeftArm, [0.05, 0.40, 0.26, 0.77]),
    (Part::RightArm, [0.74, 0.40, 0.95, 0.77]),
    (Part::LeftLeg, [0.29, 0.00, 0.49, 0.44]),
    (Part::RightLeg, [0.51, 0.00, 0.71, 0.44]),
];

/// Where a part is drawn, and so where it is clicked.
pub fn part_rect(bounds: Rect, part: Part) -> Rect {
    let [x0, y0, x1, y1] = LAYOUT
        .iter()
        .find(|(p, _)| *p == part)
        .map(|(_, f)| *f)
        .unwrap_or([0.0; 4]);
    let (w, h) = (bounds.width(), bounds.height());
    Rect::new(
        bounds.x0 + x0 * w,
        bounds.y0 + y0 * h,
        bounds.x0 + x1 * w,
        bounds.y0 + y1 * h,
    )
}

/// Which part a point is over. The inverse of `part_rect`, and nothing
/// more: the same rectangles, asked the same question.
pub fn part_at(bounds: Rect, cursor: (f32, f32)) -> Option<Part> {
    Part::ALL
        .into_iter()
        .find(|&part| part_rect(bounds, part).contains(cursor.0, cursor.1))
}

/// What the part is called.
///
/// The head and the body are the equipment squares' own words, because
/// they are the same parts of the same person; a figure whose chest was
/// called something else from the square beside it would be two answers
/// to one question.
pub fn part_name(part: Part) -> Msg {
    match part {
        Part::Head => Msg::SlotHead,
        Part::Torso => Msg::SlotChest,
        Part::LeftArm => Msg::PartLeftArm,
        Part::RightArm => Msg::PartRightArm,
        Part::LeftLeg => Msg::PartLeftLeg,
        Part::RightLeg => Msg::PartRightLeg,
    }
}

fn kind_name(kind: Kind) -> Msg {
    match kind {
        Kind::Cut => Msg::WoundCut,
        Kind::Bruise => Msg::WoundBruise,
        Kind::Fracture => Msg::WoundFracture,
        Kind::Burn => Msg::WoundBurn,
    }
}

/// How bad, in a word. **Three words and never a number**: a severity of
/// 0.43 is a countdown, and a countdown turns a wound into a progress bar
/// to wait out rather than a thing to do something about.
fn severity_word(severity: f32) -> Msg {
    if severity < 0.35 {
        Msg::SeverityLight
    } else if severity < 0.7 {
        Msg::SeveritySerious
    } else {
        Msg::SeveritySevere
    }
}

/// What the wound needs, or what is being done for it.
///
/// **The instruction is the point of the tooltip.** "Cut, serious" tells a
/// player what happened; "bleeding: needs a bandage" tells them what to
/// make, which is the question they opened the pack with.
fn state_word(kind: Kind, wound: Wound) -> Msg {
    match (kind, wound.dressed) {
        (Kind::Cut | Kind::Burn, Some(Treatment::Bandage)) => Msg::WoundBandaged,
        (_, Some(Treatment::Splint)) => Msg::WoundSplinted,
        (_, Some(Treatment::Poultice)) => Msg::WoundPoultice,
        (_, Some(Treatment::WillowBark)) => Msg::WoundBarked,
        (_, Some(Treatment::Bandage)) => Msg::WoundBandaged,
        (_, None) if heals_alone(kind, wound) => Msg::WoundHealsAlone,
        (Kind::Cut, None) => Msg::WoundBleeding,
        (Kind::Fracture, None) => Msg::WoundNeedsSplint,
        (Kind::Burn, None) => Msg::WoundNeedsDressing,
        // A bruise always heals alone, so it has been answered above.
        (Kind::Bruise, None) => Msg::WoundHealsAlone,
    }
}

const NOTE_INK: [f32; 4] = crate::ui::widgets::Theme::DARK.ink;
const NOTE_DIM: [f32; 4] = crate::ui::widgets::Theme::DARK.ink_dim;
const NOTE_GOOD: [f32; 4] = [0.52, 0.88, 0.55, 1.0];
const NOTE_BAD: [f32; 4] = [1.00, 0.48, 0.42, 1.0];

/// The lines a tooltip over a part says: its name, then a line per wound,
/// worst first, each in the colour of whether it needs something.
pub fn lines(part: Part, injuries: &Injuries, language: Language) -> Vec<(String, [f32; 4])> {
    let state = injuries.part(part);
    let mut wounds: Vec<(Kind, Wound)> = Kind::ALL
        .iter()
        .map(|&kind| (kind, state.wound(kind)))
        .filter(|(_, wound)| wound.is_open())
        .collect();
    wounds.sort_by(|a, b| {
        primitive_shared::injury::danger(b.0, b.1).total_cmp(&primitive_shared::injury::danger(a.0, a.1))
    });
    let mut out = vec![(language.text(part_name(part)).to_string(), NOTE_INK)];
    if wounds.is_empty() {
        out.push((language.text(Msg::NoWounds).to_string(), NOTE_DIM));
        return out;
    }
    for (kind, wound) in wounds {
        let needs = !wound.is_dressed() && !heals_alone(kind, wound);
        out.push((
            format!(
                "{} - {}: {}",
                language.text(kind_name(kind)),
                language.text(severity_word(wound.severity)),
                language.text(state_word(kind, wound)),
            ),
            if needs {
                NOTE_BAD
            } else if wound.is_dressed() {
                NOTE_GOOD
            } else {
                NOTE_DIM
            },
        ));
    }
    out
}

// ---- drawing ----

/// A part with nothing wrong with it: a pale, flat stone-grey, lighter
/// than the tray it stands in and darker than the writing on the panel,
/// so the figure is plainly there without competing with the pack.
pub const WHOLE: [f32; 4] = [0.46, 0.42, 0.37, 1.0];
/// The hairline round a part.
const OUTLINE: [f32; 4] = [0.03, 0.025, 0.02, 0.95];

/// The colour of a kind of wound at its worst, undressed and dressed.
///
/// **A dressed wound is the colour of its dressing**, not a paler red: a
/// bandaged arm is white on the figure the way it is white on a person,
/// and a player scanning for "what is still wrong" looks for red and
/// purple and orange and finds only the parts nobody has seen to.
fn kind_colour(kind: Kind, dressed: Option<Treatment>) -> [f32; 3] {
    match (kind, dressed) {
        (_, Some(Treatment::Bandage)) => [0.88, 0.84, 0.74],
        (_, Some(Treatment::Splint)) => [0.66, 0.50, 0.30],
        (_, Some(Treatment::Poultice)) => [0.46, 0.62, 0.30],
        // Willow bark: the grey-green of the inner bark, told from the
        // poultice's leaf green by being paler and greyer.
        (_, Some(Treatment::WillowBark)) => [0.62, 0.64, 0.50],
        (Kind::Cut, None) => [0.80, 0.14, 0.12],
        (Kind::Bruise, None) => [0.40, 0.38, 0.74],
        (Kind::Fracture, None) => [0.62, 0.20, 0.62],
        (Kind::Burn, None) => [0.96, 0.52, 0.12],
    }
}

/// What a part is filled with.
///
/// The worst wound's colour, mixed into the whole grey by how bad it is --
/// **which is what makes the mending visible**: a bandaged cut fades from
/// white back to grey as it closes, a little every time the server says
/// so, and a player watching the pack sees the arm get better rather than
/// being told it has.
pub fn fill(injuries: &Injuries, part: Part) -> [f32; 4] {
    let Some((kind, wound)) = injuries.part(part).worst() else {
        return WHOLE;
    };
    let target = kind_colour(kind, wound.dressed);
    let t = (0.35 + 0.65 * wound.severity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    [
        WHOLE[0] + (target[0] - WHOLE[0]) * t,
        WHOLE[1] + (target[1] - WHOLE[1]) * t,
        WHOLE[2] + (target[2] - WHOLE[2]) * t,
        1.0,
    ]
}

/// A mark per kind, five cells square, read from the top -- the format the
/// HUD's own marks use (`hud::Icon`), for the same reason: the picture in
/// the source is the picture on the screen.
fn mark(kind: Kind) -> [u8; 5] {
    match kind {
        // A slash.
        Kind::Cut => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000],
        // A ring.
        Kind::Bruise => [0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
        // A crack, zigzagging down.
        Kind::Fracture => [0b00100, 0b01000, 0b00100, 0b00010, 0b00100],
        // A flame.
        Kind::Burn => [0b00100, 0b01100, 0b01110, 0b11111, 0b01110],
    }
}

fn draw_mark(p: &mut Painter, bits: [u8; 5], centre: (f32, f32), size: f32, ink: [f32; 4]) {
    let cell = size / 5.0;
    let (left, top) = (centre.0 - size / 2.0, centre.1 + size / 2.0);
    for (row, bits) in bits.iter().enumerate() {
        let y1 = top - cell * row as f32;
        let mut column = 0;
        while column < 5 {
            if bits & (1 << (4 - column)) == 0 {
                column += 1;
                continue;
            }
            let start = column;
            while column < 5 && bits & (1 << (4 - column)) != 0 {
                column += 1;
            }
            p.quad(
                Rect::new(left + cell * start as f32, y1 - cell, left + cell * column as f32, y1),
                ink,
            );
        }
    }
}

/// The edge a part is given while a dressing that would help it is in
/// hand, and while the pointer is on it.
const WOULD_HELP: [f32; 4] = [0.52, 0.88, 0.55, 1.0];
const WOULD_NOT: [f32; 4] = [1.00, 0.48, 0.42, 1.0];

/// Draws the figure.
///
/// `holding` is what is on the pointer, if anything. **A dressing in hand
/// lights the parts it would help** -- the answer to "where does this go"
/// before the drop rather than after it, which is the difference between
/// a refusal the player could have seen coming and one that reads as the
/// game being arbitrary. Only advice: the server asks again, and a part
/// lit here that the server refuses is a bug in one of the two copies,
/// not a bandage spent.
pub fn draw(
    p: &mut Painter,
    bounds: Rect,
    injuries: &Injuries,
    hovered: Option<Part>,
    holding: Option<BlockId>,
) {
    let dressing = holding.and_then(Treatment::of);
    for part in Part::ALL {
        let rect = part_rect(bounds, part);
        p.quad(rect, fill(injuries, part));
        // Asked of a copy: `treat` changes what it is given on success, and
        // this is a question, not a treatment.
        let helps = holding.is_some_and(|block| {
            let mut probe = *injuries;
            dressing.is_some() && probe.treat(part, block).is_ok()
        });
        let edge = match (hovered == Some(part), dressing.is_some(), helps) {
            (_, true, true) => (0.005, WOULD_HELP),
            (true, true, false) => (0.004, WOULD_NOT),
            (true, false, _) => (0.004, crate::ui::widgets::ACCENT),
            _ => (0.0025, OUTLINE),
        };
        p.border(rect, edge.0, edge.1);

        // A mark per open wound, strung along the part's long side.
        let open: Vec<(Kind, Wound)> = Kind::ALL
            .iter()
            .map(|&kind| (kind, injuries.part(part).wound(kind)))
            .filter(|(_, wound)| wound.is_open())
            .collect();
        if open.is_empty() {
            continue;
        }
        let tall = rect.height() >= rect.width();
        let along = if tall { rect.height() } else { rect.width() };
        let across = if tall { rect.width() } else { rect.height() };
        let size = (across * 0.6).min(along / open.len() as f32 * 0.7).min(0.03);
        for (n, (kind, wound)) in open.iter().enumerate() {
            let t = (n as f32 + 0.5) / open.len() as f32;
            let centre = if tall {
                (rect.centre_x(), rect.y1 - along * t)
            } else {
                (rect.x0 + along * t, rect.centre_y())
            };
            // Dark on a pale dressing, pale on an open wound.
            let ink = if wound.is_dressed() {
                [0.16, 0.12, 0.08, 0.9]
            } else {
                [0.98, 0.96, 0.90, 1.0]
            };
            draw_mark(p, mark(*kind), centre, size, ink);
        }
    }
}

// ---- what is said out loud ----

fn fractures(injuries: &Injuries) -> usize {
    Part::ALL
        .iter()
        .filter(|&&part| injuries.wound(part, Kind::Fracture).is_open())
        .count()
}

fn bad_burns(injuries: &Injuries) -> usize {
    Part::ALL
        .iter()
        .filter(|&&part| {
            let burn = injuries.wound(part, Kind::Burn);
            burn.is_open() && !burn.is_dressed() && !heals_alone(Kind::Burn, burn)
        })
        .count()
}

/// The one line worth putting on screen when the body changed from
/// `before` to `after`, if any.
///
/// **One line, the most urgent.** A bear that breaks an arm and opens it
/// in the same blow has done two things, and the one that is killing the
/// player is the one to say; the other is on the mannequin. Bad news
/// before good, and a new wound before an old one mending.
pub fn notice(before: &Injuries, after: &Injuries) -> Option<Msg> {
    if after.is_bleeding() && !before.is_bleeding() {
        return Some(Msg::NoticeBleeding);
    }
    if fractures(after) > fractures(before) {
        return Some(Msg::NoticeBoneBroken);
    }
    if bad_burns(after) > bad_burns(before) {
        return Some(Msg::NoticeBadBurn);
    }
    if fractures(after) < fractures(before) {
        return Some(Msg::NoticeBoneKnit);
    }
    if before.is_bleeding() && !after.is_bleeding() {
        return Some(Msg::NoticeBleedingStopped);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_BANDAGE, BLOCK_SPLINT, BLOCK_STONE};

    fn boxes() -> [Rect; 3] {
        [
            Rect::new(-0.9, -0.2, -0.6, 0.25),
            Rect::new(0.0, 0.0, 0.3, 0.447),
            Rect::new(-2.0, -1.0, 1.0, 1.0),
        ]
    }

    #[test]
    fn every_part_is_clicked_where_it_is_drawn_and_nowhere_else() {
        for bounds in boxes() {
            for part in Part::ALL {
                let r = part_rect(bounds, part);
                assert!(r.width() > 0.0 && r.height() > 0.0, "{part:?} has no area");
                assert!(
                    r.x0 >= bounds.x0 && r.x1 <= bounds.x1 && r.y0 >= bounds.y0 && r.y1 <= bounds.y1,
                    "{part:?} hangs out of the figure"
                );
                // The middle and every corner, a hair inside.
                let e = 1e-4;
                for point in [
                    (r.centre_x(), r.centre_y()),
                    (r.x0 + e, r.y0 + e),
                    (r.x1 - e, r.y0 + e),
                    (r.x0 + e, r.y1 - e),
                    (r.x1 - e, r.y1 - e),
                ] {
                    assert_eq!(part_at(bounds, point), Some(part), "{part:?} misses itself at {point:?}");
                }
                // ...and a hair outside is not this part.
                for point in [(r.x0 - e, r.centre_y()), (r.centre_x(), r.y1 + e)] {
                    assert_ne!(part_at(bounds, point), Some(part), "{part:?} answers past its edge");
                }
            }
            for (n, a) in Part::ALL.iter().enumerate() {
                for b in &Part::ALL[n + 1..] {
                    let (ra, rb) = (part_rect(bounds, *a), part_rect(bounds, *b));
                    let overlaps = ra.x0 < rb.x1 && ra.x1 > rb.x0 && ra.y0 < rb.y1 && ra.y1 > rb.y0;
                    assert!(!overlaps, "{a:?} and {b:?} share a patch of the figure");
                }
            }
            // The gap between the head and the body is nobody's.
            let neck = (bounds.x0 + bounds.width() * 0.5, bounds.y0 + bounds.height() * 0.785);
            assert_eq!(part_at(bounds, neck), None, "the neck is somebody's");
        }
    }

    #[test]
    fn every_part_has_a_name_of_its_own_in_every_language() {
        for &language in Language::ALL {
            let names: Vec<&str> = Part::ALL.iter().map(|&p| language.text(part_name(p))).collect();
            for (n, name) in names.iter().enumerate() {
                assert!(!name.is_empty());
                assert!(!names[n + 1..].contains(name), "{language:?}: two parts are both {name}");
            }
        }
    }

    #[test]
    fn a_wound_says_what_to_do_about_it() {
        let english = Language::English;
        let say = |msg| english.text(msg);
        let mut body = Injuries::default();
        assert!(lines(Part::LeftArm, &body, english)[1].0.contains(say(Msg::NoWounds)));

        body.inflict(Part::LeftArm, Kind::Cut, 0.8);
        let open = lines(Part::LeftArm, &body, english);
        assert!(open[1].0.contains(say(Msg::WoundBleeding)), "{open:?}");
        assert_eq!(open[1].1, NOTE_BAD, "an open cut was not drawn as a thing to act on");

        body.treat(Part::LeftArm, BLOCK_BANDAGE).expect("fits");
        let dressed = lines(Part::LeftArm, &body, english);
        assert!(dressed[1].0.contains(say(Msg::WoundBandaged)), "{dressed:?}");

        body.inflict(Part::LeftLeg, Kind::Fracture, 1.0);
        body.inflict(Part::LeftLeg, Kind::Bruise, 0.2);
        let leg = lines(Part::LeftLeg, &body, english);
        assert!(leg[1].0.contains(say(Msg::WoundNeedsSplint)), "the worst wound was not first: {leg:?}");
        assert!(leg[2].0.contains(say(Msg::WoundHealsAlone)));
    }

    #[test]
    fn a_worse_wound_is_drawn_stronger_and_a_mending_one_fades() {
        let distance = |c: [f32; 4]| {
            (0..3).map(|i| (c[i] - WHOLE[i]).powi(2)).sum::<f32>().sqrt()
        };
        let cut = |severity| {
            let mut body = Injuries::default();
            body.inflict(Part::Torso, Kind::Cut, severity);
            body
        };
        assert_eq!(fill(&Injuries::default(), Part::Torso), WHOLE);
        assert!(distance(fill(&cut(0.9), Part::Torso)) > distance(fill(&cut(0.2), Part::Torso)));
        // A dressed wound is not the colour of the open one.
        let mut dressed = cut(0.9);
        dressed.treat(Part::Torso, BLOCK_BANDAGE).expect("fits");
        assert_ne!(fill(&dressed, Part::Torso), fill(&cut(0.9), Part::Torso));
    }

    #[test]
    fn a_new_wound_is_said_once_and_its_mending_once() {
        let whole = Injuries::default();
        let mut bleeding = whole;
        bleeding.inflict(Part::RightArm, Kind::Cut, 0.8);
        assert_eq!(notice(&whole, &bleeding), Some(Msg::NoticeBleeding));
        assert_eq!(notice(&bleeding, &bleeding), None, "a bleed was announced twice");

        let mut dressed = bleeding;
        dressed.treat(Part::RightArm, BLOCK_BANDAGE).expect("fits");
        assert_eq!(notice(&bleeding, &dressed), Some(Msg::NoticeBleedingStopped));

        let mut broken = whole;
        broken.inflict(Part::LeftLeg, Kind::Fracture, 1.0);
        assert_eq!(notice(&whole, &broken), Some(Msg::NoticeBoneBroken));
        let mut set = broken;
        set.treat(Part::LeftLeg, BLOCK_SPLINT).expect("fits");
        assert_eq!(notice(&broken, &set), None, "setting a leg was news");
        assert_eq!(notice(&set, &whole), Some(Msg::NoticeBoneKnit));

        // The worse of two things at once.
        let mut mauled = whole;
        mauled.inflict(Part::LeftArm, Kind::Fracture, 1.0);
        mauled.inflict(Part::LeftArm, Kind::Cut, 1.0);
        assert_eq!(notice(&whole, &mauled), Some(Msg::NoticeBleeding));
    }

    #[test]
    fn a_dressing_in_hand_lights_the_parts_it_would_help_and_a_stone_lights_none() {
        use crate::engine::texture::FontAtlas;
        let bounds = Rect::new(0.0, 0.0, 0.3, 0.447);
        let mut body = Injuries::default();
        body.inflict(Part::LeftArm, Kind::Cut, 0.8);
        let edges = |holding| {
            let mut p = Painter::new(FontAtlas::for_test());
            draw(&mut p, bounds, &body, None, holding);
            p.into_vertices()
                .iter()
                .filter(|v| v.tint == WOULD_HELP)
                .count()
        };
        assert!(edges(Some(BLOCK_BANDAGE)) > 0, "a bandage over a cut arm lit nothing");
        assert_eq!(edges(Some(BLOCK_SPLINT)), 0, "a splint lit a cut arm");
        assert_eq!(edges(Some(BLOCK_STONE)), 0, "a stone lit the body");
        assert_eq!(edges(None), 0);
    }
}
