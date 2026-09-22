//! Playing without a keyboard.
//!
//! ## What this is not
//!
//! It is not a second control scheme. Everything below turns fingers
//! into exactly the state a mouse and keyboard would have produced --
//! the movement stick fills the same `InputState` that W and S fill, a
//! drag on the right of the screen accumulates the same look delta a
//! mouse does, and the dig button sets the same `breaking` flag the
//! left mouse button sets. Nothing downstream of `InputState` knows a
//! phone exists, which is the only way a control scheme for a platform
//! nobody can test daily stays working.
//!
//! ## Why the stick is analogue and the keys are not
//!
//! A keyboard has four directions and one speed. A thumb has every
//! direction and every speed, and flattening it back to four booleans
//! throws away the half of it that makes a stick feel like a stick --
//! there would be no way to walk quietly up to a deer, only to run at
//! it. So `InputState` carries an optional stick vector beside the
//! keys, and `wish_direction` prefers it when a thumb is on the glass.
//! On a desktop it is always `None` and the keys are read exactly as
//! they always were.
//!
//! ## Why the layout is in fractions of the screen
//!
//! Phones differ by a factor of three in pixel density and by a factor
//! of two in aspect. A stick placed at "160 pixels from the corner" is
//! under the thumb on one device and out of reach on the next. Placed
//! at a fraction of the *shorter* side, it lands in the same place
//! relative to the hand holding it, which is what a thumb actually
//! cares about.

use super::{Size, TouchId, TouchPhase};

/// A button drawn on the glass.
///
/// Deliberately few. Every one of these costs a piece of the world the
/// player cannot see past, and a phone screen is mostly thumb already.
/// Anything that is not needed *while moving* -- the settings, the
/// world list -- stays on the menus, where there is room and nothing to
/// aim at.
/// One of the buttons on the glass, by position in the arrangement.
///
/// **A number, not a meaning.** These used to be `Dig`, `Place`, `Jump`
/// and `Inventory` -- an enum this layer understood -- and that was the
/// thing standing between a player and a button that does what they
/// want. What a button does is now a key it carries (see
/// `settings::Emits`), so this layer's whole job is to say *which*
/// button a thumb landed on and let the game look up the rest. The
/// platform layer is not supposed to know what the game means by a
/// press; see the note on `platform` in CLAUDE.md.
pub type Slot = usize;

/// What one touch did to the buttons.
///
/// Both edges, not just the press. A button emulates a key, and a key
/// that is pressed and never released is a key stuck down -- the game
/// would keep mining, keep running, keep holding whatever it was until
/// something else happened to clear it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Pressed(Slot),
    Released(Slot),
    Nothing,
}

/// One finger, and where it is.
#[derive(Debug, Clone, Copy)]
struct Finger {
    id: TouchId,
    /// Where it went down. The stick measures from here, so the stick
    /// centres itself wherever the thumb happened to land rather than
    /// making the thumb find a painted circle.
    origin: (f32, f32),
    at: (f32, f32),
    /// When it went down, for the finger in the look area: the
    /// difference between placing and mining is how long it stays. See
    /// [`Hand`].
    down_at: Option<std::time::Instant>,
    /// Whether it has travelled far enough to be a camera drag.
    ///
    /// Once true it stays true, so a finger that wandered and came back
    /// is still a drag and does not place a block on the way up.
    dragged: bool,
    /// Whether the left button is currently down because of it.
    mining: bool,
}

/// Where everything sits, in pixels, for one screen size.
#[derive(Debug, Clone, Copy, Default)]
pub struct Layout {
    pub size: Size,
    /// The movement thumb: middle, and the half-extents a fully-pushed
    /// thumb is measured against.
    pub stick: Placed,
    /// One per slot, in arrangement order.
    pub buttons: [Placed; crate::settings::TouchLayout::BUTTONS],
    /// The arrangement these numbers were worked out from. Kept so
    /// `resize` can tell "same screen, same arrangement" -- which is
    /// every frame -- from "the player just moved a button", which the
    /// editor does while they watch and which must be seen at once.
    arrangement: crate::settings::TouchLayout,
    /// Whether the wheel was open when these numbers were worked out.
    ///
    /// Part of the layout rather than only of [`Touch`], because it is
    /// what decides whether the members are on the glass -- and the
    /// drawing and the hit-testing both read the answer from here, so
    /// they cannot disagree about it. See [`Layout::for_size`].
    wheel_open: bool,
}

/// One control's box on the glass, in pixels.
#[derive(Debug, Clone, Copy, Default)]
pub struct Placed {
    pub centre: (f32, f32),
    /// Half the width and half the height. Halves rather than the
    /// whole, because every question asked of this -- is the finger
    /// inside, how far is it pushed, where does the border go -- is
    /// asked from the middle outward.
    pub half: (f32, f32),
    pub shown: bool,
    /// What this button sends, carried through from the arrangement so
    /// that drawing it needs nothing but the layout. The alternative --
    /// handing the settings to the painter as well -- is two sources
    /// for one answer, and the drawing and the pressing then disagree
    /// the first time one of them is passed a stale copy.
    pub emits: crate::settings::Emits,
}

impl Placed {
    /// Whether a point in pixels is inside this control, allowing it
    /// `slack` pixels of reach past its own edge on every side.
    ///
    /// The one definition of "inside a control", so the generous
    /// version the game presses with and the exact version a test asks
    /// about cannot drift apart. A control that is switched off is
    /// inside nothing: that is what being off means, and saying it here
    /// saves every caller from remembering.
    pub fn contains(&self, x: f32, y: f32, slack: f32) -> bool {
        self.shown
            && (x - self.centre.0).abs() <= self.half.0 + slack
            && (y - self.centre.1).abs() <= self.half.1 + slack
    }

    /// The shorter half-extent, which is what a *radius* means for a
    /// control that need not be square: how far a thumb can be pushed
    /// in the direction that runs out first.
    pub fn radius(&self) -> f32 {
        self.half.0.min(self.half.1)
    }

    /// How far a point lies outside this control, in pixels; zero
    /// anywhere inside it.
    ///
    /// Measured to the nearest point of the rectangle rather than to
    /// the middle, which is what lets [`Layout::button_at`] compare two
    /// controls of different sizes fairly -- see the argument there.
    fn distance_outside(&self, x: f32, y: f32) -> f32 {
        let over_x = ((x - self.centre.0).abs() - self.half.0).max(0.0);
        let over_y = ((y - self.centre.1).abs() - self.half.1).max(0.0);
        over_x.hypot(over_y)
    }
}

/// How far the thumb buttons have to rise to clear the hotbar.
///
/// ## Why the bar has to be consulted at all
///
/// Because the two are laid out in different units and only one of them
/// answers to INTERFACE SIZE. A thumb control is a fraction of the
/// screen's shorter side and is deliberately *not* scaled -- it is
/// already the size of a finger, and a finger does not grow when a
/// player asks for bigger writing (see `hud::touch_controls`). The
/// hotbar is authored in interface units and *is* scaled, about the
/// bottom edge, by exactly that setting.
///
/// So the gap between them is not a constant, and a layout checked at
/// one setting is not checked at another. On the phone this was found
/// on, at INTERFACE SIZE 1.5, the bar reached 120 pixels further than
/// the arrangement expected: the tenth slot was drawn *underneath* the
/// sneak button, and tapping the slot pressed the button. No choice of
/// numbers avoids it, because at the top of the size range the bar is
/// wider than the screen leaves beside it.
///
/// ## Why one lift for the whole block
///
/// Because lifting each button by what *it* needs closes the gaps
/// between them. Only the bottom row of the block sits on the bar; move
/// that row alone and it rises into the row above -- measured, at 1.5
/// they ended up overlapping by 35 pixels, which trades a covered slot
/// for two buttons that cannot be told apart. One lift for every
/// control that shares the bar's span keeps the arrangement the shape
/// the player sees, and simply puts it higher.
///
/// ## Why the buttons and not the stick
///
/// The buttons are one block and have to stay one: lifting only the
/// column that happens to overlap leaves a stepped, ragged arrangement
/// that reads as a mistake -- which is what the first version of this
/// did, and it looked worse than the bug it fixed. So the worst
/// overlap decides one lift and every button takes it.
///
/// The stick is not a button and is left alone. It sits in the
/// bottom-left corner, clear of a centred bar, and taking the corner a
/// left thumb rests in would cost something and buy nothing.
fn lift_clear_of_the_bar(buttons: &mut [Placed], size: Size, ui_scale: f32) {
    use crate::ui::hotbar;

    let size = size.non_zero();
    let (w, h) = (size.width as f32, size.height as f32);
    // Interface units to pixels: y spans -1..1 over the height, and x is
    // in units of height measured from the middle.
    let per_unit = h / 2.0;
    let scale = if ui_scale.is_finite() && ui_scale > 0.0 { ui_scale } else { 1.0 };

    // The bar as it is actually drawn: grown about the bottom edge, the
    // way `scale_about` with `anchor::BOTTOM` grows it.
    let grown = |y: f32| -1.0 + (y + 1.0) * scale;
    let half_width = hotbar::RIGHT * scale * per_unit;
    let bar_left = w / 2.0 - half_width;
    let bar_right = w / 2.0 + half_width;
    // Pixels run down the screen and interface units run up it.
    let bar_top = (1.0 - grown(hotbar::TOP)) * per_unit;

    let over_the_bar = |control: &Placed| {
        let (left, right) = (control.centre.0 - control.half.0, control.centre.0 + control.half.0);
        right > bar_left && left < bar_right
    };

    // The worst offender decides the lift...
    let mut lift: f32 = 0.0;
    for button in buttons.iter() {
        if !button.shown || !over_the_bar(button) || button.centre.1 <= h / 2.0 {
            continue;
        }
        let bottom = button.centre.1 + button.half.1;
        lift = lift.max(bottom - bar_top);
    }
    if lift <= 0.0 {
        return;
    }
    // ...and every button *in the bottom half* takes it, so the block
    // down there keeps its shape while the row along the top edge stays
    // where it was put. Lifting those too would march them off the
    // glass to solve a collision they were never in.
    let bottom_half = |button: &Placed| button.centre.1 > h / 2.0;
    for button in buttons.iter_mut().filter(|b| bottom_half(b)) {
        // Never off the top of the glass: a covered slot is a nuisance
        // and an unreachable button is a dead control.
        button.centre.1 = (button.centre.1 - lift).max(button.half.1);
    }
}

/// Keeps a control inside the glass.
///
/// A control wider than the glass cannot be kept inside it, and
/// clamping one anyway would pin it to the left edge and look like the
/// arrangement was ignored. Centre it instead: that is visibly "too
/// big", which is the truth.
///
/// A free function rather than a closure inside `for_size` because the
/// wheel needs it too, and a second copy of a clamp is a second answer
/// to "is this on screen".
fn keep_inside(value: f32, span: f32, half: f32) -> f32 {
    if half * 2.0 >= span {
        span / 2.0
    } else {
        value.clamp(half, span - half)
    }
}

/// Where one control goes, in pixels, given how it is written down.
///
/// **Lifted out of `Layout::for_size` so that it can be inverted.** The
/// editor drags a control to a point on the glass and has to write down
/// an arrangement that puts it back at that same point; a conversion
/// that is off by anything at all is a button that jumps out from under
/// the finger the moment it is let go. Having one function for the
/// forward direction means [`placement_for`] has something exact to be
/// the inverse of, and there is a test that round-trips the pair.
fn place_one(placement: &crate::settings::Placement, size: Size) -> Placed {
    let size = size.non_zero();
    let (w, h) = (size.width as f32, size.height as f32);
    let short = w.min(h);
    let (half_w, half_h) = placement.half();
    let (half_w, half_h) = (half_w * short, half_h * short);
    let (in_x, in_y) = (placement.inset.0 * short, placement.inset.1 * short);

    // The inset counts inward from the placement's own corner, so which
    // edge it counts from is part of the arrangement rather than a rule
    // about where buttons go.
    let x = if placement.corner.counts_from_left() { in_x } else { w - in_x };
    let y = if placement.corner.counts_from_top() { in_y } else { h - in_y };

    Placed {
        centre: (keep_inside(x, w, half_w), keep_inside(y, h, half_h)),
        half: (half_w, half_h),
        shown: placement.shown,
        emits: placement.emits,
    }
}

/// How to write down a control that has been dragged to a point.
///
/// **The exact inverse of [`place_one`]**, and it has to be exact: the
/// editor moves a control with a finger and the game then draws it from
/// the arrangement, so any drift at all is a control that jumps when it
/// is released.
///
/// The corner is chosen by which quarter of the glass the control
/// landed in, and that is the whole reason a corner is part of a
/// placement at all (see `settings::Corner`): a button dropped on the
/// right is measured from the right, so it stays on the right when the
/// phone is turned. Dropping it across the middle re-corners it, which
/// is what a player moving a button from one thumb to the other means.
///
/// Clamped here rather than left to `place_one`, so that what the
/// arrangement says and where the control ends up are the same point: a
/// control dragged off the edge settles against the edge, which is
/// where it was being dragged, rather than snapping back.
pub fn placement_for(
    centre: (f32, f32),
    size: Size,
    was: crate::settings::Placement,
) -> crate::settings::Placement {
    use crate::settings::Corner;
    let size = size.non_zero();
    let (w, h) = (size.width as f32, size.height as f32);
    let short = w.min(h);
    let (half_w, half_h) = was.half();
    let (half_w, half_h) = (half_w * short, half_h * short);
    let x = keep_inside(centre.0, w, half_w);
    let y = keep_inside(centre.1, h, half_h);
    let corner = match (x * 2.0 <= w, y * 2.0 <= h) {
        (true, true) => Corner::TopLeft,
        (false, true) => Corner::TopRight,
        (true, false) => Corner::BottomLeft,
        (false, false) => Corner::BottomRight,
    };
    crate::settings::Placement {
        corner,
        inset: (
            if corner.counts_from_left() { x } else { w - x } / short,
            if corner.counts_from_top() { y } else { h - y } / short,
        ),
        ..was
    }
}

/// The least air there may be between two controls, as a fraction of
/// the screen's shorter side.
///
/// **Between what a player sees, not between what is hit-tested.** A
/// control's frame is drawn a stroke and a half outside its own box on
/// every side (`hud::frame_overhang`), and the arrangement used to be
/// spaced without knowing that: the buttons along the top edge stood 33
/// px apart on the phone this is played on and the frames closed all
/// but six of it. Six pixels on a 2712-pixel screen is not a gap. The
/// player's picture showed three buttons reading as one bar, with the
/// word on one running into the line of the next.
///
/// A hundredth of the shorter side, which on that phone is 12 px and on
/// any phone is about the width of the strokes it is separating.
const CLEARANCE: f32 = 0.01;

/// How much room a control takes on the glass, frame and all.
fn drawn_half(placed: &Placed) -> (f32, f32) {
    let over = crate::ui::hud::frame_overhang(placed.radius());
    (placed.half.0 + over, placed.half.1 + over)
}

/// Whether two controls are close enough to read as one.
///
/// The one definition, so the rule that spaces the wheel, the rule that
/// takes a button off the glass and the test that says neither of them
/// missed anything are all asking the same question.
fn crowds(one: &Placed, two: &Placed, short: f32) -> bool {
    let (one_x, one_y) = drawn_half(one);
    let (two_x, two_y) = drawn_half(two);
    let air = short * CLEARANCE;
    (one.centre.0 - two.centre.0).abs() < one_x + two_x + air
        && (one.centre.1 - two.centre.1).abs() < one_y + two_y + air
}

/// Air between the wheel's hub and the ring of members around it, as a
/// fraction of the screen's shorter side.
///
/// Enough that hub and member do not touch -- two frames sharing an
/// edge read as one shape -- and no more, because every pixel further
/// out is a member further from the thumb that opened the wheel.
const WHEEL_GAP: f32 = 0.03;

/// Arranges the wheel's members around the button that opens it.
///
/// ## Why they are placed here and not by the player
///
/// Everything else on the glass is where the arrangement says. A wheel
/// member is not: what makes a wheel readable is that its members sit
/// evenly around one middle, and an arrangement that let three of them
/// be dragged apart would be an arrangement that could make the wheel
/// stop looking like one. So the member keeps its size and loses its
/// place. See `settings::Placement::in_wheel`.
///
/// ## Why the arc points inward
///
/// The hub sits in a corner -- that is the whole point of it, being
/// out of the way of the two thumbs -- so half of any ring drawn round
/// it would be off the glass. The quarter turn from "along the near
/// edge" to "straight into the screen" is the only quarter that is
/// entirely on it, whichever corner the hub is in, and the corner is
/// something the placement already knows.
///
/// **With no hub there is no wheel.** If the player has switched the
/// `...` button off, its members go back to being ordinary buttons at
/// their own insets rather than becoming unreachable -- a control that
/// exists, is switched on and can never be pressed is the worst of the
/// three states it could be in.
fn arrange_the_wheel(
    buttons: &mut [Placed],
    arrangement: &crate::settings::TouchLayout,
    size: Size,
    open: bool,
) {
    let (w, h) = (size.width as f32, size.height as f32);
    let short = w.min(h);

    let Some(hub) = (0..buttons.len()).find(|slot| {
        buttons[*slot].shown && matches!(buttons[*slot].emits, crate::settings::Emits::More)
    }) else {
        return;
    };
    let members: Vec<usize> = (0..buttons.len())
        .filter(|slot| arrangement.buttons[*slot].in_wheel && buttons[*slot].shown)
        .collect();
    if members.is_empty() {
        return;
    }

    // The ring clears the widest member, so the members sit on a circle
    // rather than each at its own distance -- which is what a wheel
    // with one big button on it would otherwise look like.
    let widest = members
        .iter()
        .map(|slot| buttons[*slot].radius())
        .fold(0.0f32, f32::max);
    // Every distance here is between *drawn* edges: what has to not
    // touch is what a player can see. See [`CLEARANCE`].
    let hub_reach = buttons[hub].radius() + crate::ui::hud::frame_overhang(buttons[hub].radius());
    let member_reach = widest + crate::ui::hud::frame_overhang(widest);
    let air = short * WHEEL_GAP;

    // A lone member sits on the diagonal; with more than one they span
    // the quarter turn end to end, so the first lies along the edge and
    // the last points straight into the screen.
    let angle_of = |index: usize| {
        let t = if members.len() == 1 {
            0.5
        } else {
            index as f32 / (members.len() - 1) as f32
        };
        t * std::f32::consts::FRAC_PI_2
    };

    // Wide enough to clear the hub -- and **divided by how much of the
    // ring each direction actually buys**, which the first cut of this
    // missed. Two boxes miss each other when they are far enough apart
    // on either axis, and a member on the diagonal is only `cos 45` of
    // the ring away from the hub on each of them: it sat nine pixels
    // inside the button that had opened it.
    let hub_factor = (0..members.len())
        .map(|index| {
            let angle = angle_of(index);
            angle.cos().max(angle.sin())
        })
        .fold(f32::INFINITY, f32::min);
    let clear_of_the_hub = if hub_factor.is_finite() && hub_factor > 0.0 {
        (hub_reach + member_reach + air) / hub_factor
    } else {
        hub_reach + member_reach + air
    };

    // ...and wide enough that the members clear *each other*, which is
    // a different sum and the one that usually binds.
    //
    // **Clearing the hub is not enough, and the first cut of this got
    // it wrong.** Three buttons 220 px across, on a ring only wide
    // enough to miss the hub, are 196 px apart along the arc: they
    // overlapped by a quarter of their width, so the middle of the
    // wheel was two frames drawn through one another and a tap in the
    // overlap belonged to whichever `button_at` reached first.
    //
    // The controls are rectangles, so the sum is a rectangle's: two of
    // them miss each other when they are far enough apart on *either*
    // axis, which is why this takes the larger of the two differences
    // rather than the distance between the centres. Solved for the
    // closest adjacent pair, which for an arc that starts on the edge
    // is the pair nearest the diagonal.
    let closest = (1..members.len())
        .map(|index| {
            let (a, b) = (angle_of(index - 1), angle_of(index));
            (a.cos() - b.cos()).abs().max((a.sin() - b.sin()).abs())
        })
        .fold(f32::INFINITY, f32::min);
    let clear_of_each_other = if closest.is_finite() && closest > 0.0 {
        (member_reach * 2.0 + air) / closest
    } else {
        0.0
    };
    let ring = clear_of_the_hub.max(clear_of_each_other);

    // Into the screen, away from whichever corner the hub is measured
    // from. Pixels run down the glass, so "counted from the top" and
    // "downward" are the same sign.
    let corner = arrangement.buttons[hub].corner;
    let inward_x = if corner.counts_from_left() { 1.0 } else { -1.0 };
    let inward_y = if corner.counts_from_top() { 1.0 } else { -1.0 };

    let (cx, cy) = buttons[hub].centre;
    for (index, slot) in members.iter().enumerate() {
        let angle = angle_of(index);
        let (half_w, half_h) = buttons[*slot].half;
        buttons[*slot].centre = (
            keep_inside(cx + inward_x * ring * angle.cos(), w, half_w),
            keep_inside(cy + inward_y * ring * angle.sin(), h, half_h),
        );
        // **Drawn and pressed by the same flag.** A member hit-tested
        // while invisible would be a button that opens the chat box out
        // of an empty corner, which is exactly the mis-aimed swing the
        // arrangement is laid out to avoid.
        buttons[*slot].shown = open;
    }
}

/// Takes off the glass anything an open wheel is standing on.
///
/// ## The failure, which a player found before a test did
///
/// The buttons along the top edge were drawn overlapping on a phone at
/// INTERFACE SIZE 1.45: one frame across another, and a word cut off by
/// the frame standing on it. That particular row is inside the wheel
/// now, and the *shape* of the fault is not: the wheel reaches inward
/// from a corner, and INTERFACE SIZE moves the permanent buttons --
/// `lift_clear_of_the_bar` pushes them up as the hotbar grows. At the
/// top of the setting's range the two meet, measured: JUMP lifted to
/// 734 px and the wheel's last member reaching down to 655, thirty
/// pixels of one drawn through the other.
///
/// ## Why the wheel wins and the button goes
///
/// Because one of them is a menu the player has just opened and the
/// other is a control for a game they are not, at that moment, playing.
/// A wheel that shuffled its members out of the way instead would be a
/// wheel whose members are somewhere different depending on the
/// interface size, which is the one thing a menu you aim at by feel
/// must not be.
///
/// Nothing is lost: what is hidden is not drawn and not pressed -- the
/// same flag, so they cannot disagree -- and it is back the instant the
/// wheel is dismissed, which is one touch anywhere.
fn yield_to_the_wheel(
    buttons: &mut [Placed],
    arrangement: &crate::settings::TouchLayout,
    short: f32,
) {
    let members: Vec<Placed> = (0..buttons.len())
        .filter(|slot| arrangement.buttons[*slot].in_wheel && buttons[*slot].shown)
        .map(|slot| buttons[slot])
        .collect();
    if members.is_empty() {
        return;
    }
    for (slot, button) in buttons.iter_mut().enumerate() {
        if arrangement.buttons[slot].in_wheel || !button.shown {
            continue;
        }
        if members.iter().any(|member| crowds(member, button, short)) {
            button.shown = false;
        }
    }
}

impl Layout {
    /// Lays the controls out for a screen.
    ///
    /// Everything is a fraction of the *shorter* side. A phone held
    /// sideways is much wider than it is tall, and sizing a thumb
    /// control against the long side makes it enormous.
    ///
    /// The arrangement arrives from the player, through the editor or
    /// through a hand-edited file, so this is also the last place that
    /// can refuse a control which would land off the glass. It refuses
    /// by clamping rather than by falling back to the default: a button
    /// dragged a little too far should end up against the edge, which
    /// is where it was being dragged, and not jump back to where it
    /// started.
    pub fn for_size(
        size: Size,
        arrangement: crate::settings::TouchLayout,
        ui_scale: f32,
        wheel_open: bool,
    ) -> Self {
        let size = size.non_zero();
        let (w, h) = (size.width as f32, size.height as f32);
        let short = w.min(h);

        let _ = (w, h, short);
        let mut buttons: [Placed; crate::settings::TouchLayout::BUTTONS] =
            std::array::from_fn(|index| place_one(&arrangement.buttons[index], size));
        let stick = place_one(&arrangement.stick, size);

        // Before the lift, because the lift is a rule about where a
        // button ended up and a wheel member has not ended up anywhere
        // until the wheel has put it there.
        arrange_the_wheel(&mut buttons, &arrangement, size, wheel_open);

        // Then the lift, because it is a rule about where the buttons
        // ended up rather than about where they were asked to go.
        lift_clear_of_the_bar(&mut buttons, size, ui_scale);

        // And last of all, because it is the only rule that needs every
        // other one to have finished: it is about what is standing
        // where, and the lift is the thing that moves buttons after
        // they have been placed.
        yield_to_the_wheel(
            &mut buttons,
            &arrangement,
            (size.width.min(size.height)) as f32,
        );

        Self {
            size,
            stick,
            buttons,
            arrangement,
            wheel_open,
        }
    }

    /// Which slot opens the wheel, if the player has one.
    fn hub(&self) -> Option<Slot> {
        (0..self.buttons.len()).find(|slot| {
            self.buttons[*slot].shown
                && matches!(self.buttons[*slot].emits, crate::settings::Emits::More)
        })
    }

    /// Whether this slot is one of the wheel's members.
    ///
    /// Asked of the arrangement rather than of the placed control,
    /// because a member that is closed is not on the glass and still
    /// has to be recognised as the thing that closes the wheel when a
    /// finger comes off it.
    fn is_wheel_member(&self, slot: Slot) -> bool {
        self.arrangement
            .buttons
            .get(slot)
            .is_some_and(|placement| placement.in_wheel)
            && self.hub().is_some()
    }

    /// Which button is under a point, if any.
    ///
    /// Hit-tested larger than it is drawn. A thumb is about ten
    /// millimetres wide and cannot see what is under it; a button that
    /// is exactly as big as it looks is a button that gets missed.
    ///
    /// The reach is added to each half-extent rather than multiplied
    /// in, so a control the player has made small still gains a whole
    /// finger's worth of slack instead of a quarter of not very much.
    ///
    /// ## Why the nearest one and not the first one
    ///
    /// Because the reach makes neighbours overlap, and this used to
    /// answer with whichever of them had the lower slot number.
    ///
    /// In the default arrangement the buttons in a column stand
    /// `0.207` of the short side apart and are `0.18` across, leaving
    /// `0.027` of bare gap; each side of that gap then grows `0.022`
    /// of reach toward the other, so the two claims overlap by `0.017`
    /// of the short side. On the phone this was measured on that is a
    /// band 21 pixels tall between every pair, and `find` gave all of
    /// it to the lower slot -- which is *mine*, sitting under *place*.
    /// A thumb aimed at the bottom of place, nearer to place than to
    /// mine, dug the block out instead of putting one down.
    ///
    /// So: a button hit inside its own drawn edges is that button and
    /// no argument, and only a touch that missed every button is given
    /// to the nearest one still reaching for it. Distance is measured
    /// to the button's *edge* rather than to its centre, because the
    /// controls are not all one size -- a wide button's centre is far
    /// away even when the thumb is resting on its border.
    fn button_at(&self, x: f32, y: f32) -> Option<Slot> {
        // Drawn-size first, with no slack at all: what a player can see
        // they are touching outranks what a neighbour can reach for.
        if let Some(slot) = (0..self.buttons.len())
            .find(|slot| self.buttons[*slot].contains(x, y, 0.0))
        {
            return Some(slot);
        }
        let slack = self.size.non_zero().shorter() as f32 * REACH;
        (0..self.buttons.len())
            .filter(|slot| self.buttons[*slot].contains(x, y, slack))
            .min_by(|a, b| {
                self.buttons[*a]
                    .distance_outside(x, y)
                    .total_cmp(&self.buttons[*b].distance_outside(x, y))
            })
    }

    /// Whether a thumb landing here is turning the camera rather than
    /// walking.
    ///
    /// **The half of the screen opposite the stick.** It used to be the
    /// right half, full stop, which was the same thing back when the
    /// stick was always bottom-left. Now that the player places it, a
    /// fixed rule would put the look area on top of the stick for
    /// anyone left-handed enough to move it -- and a thumb on the stick
    /// would turn the camera instead of walking.
    ///
    /// Which half the stick is in is decided by its middle, so a stick
    /// dragged exactly to the centre line falls on the right and looking
    /// goes left. Either answer is arbitrary there; having one is not.
    fn is_look_area(&self, x: f32, _y: f32) -> bool {
        let middle = self.size.non_zero().width as f32 / 2.0;
        if self.stick.centre.0 < middle {
            x >= middle
        } else {
            x < middle
        }
    }
}

/// What the hand in the look area asked for.
///
/// ## Why the hands are a gesture and not two buttons
///
/// They were two buttons, `MINE` and `PLACE`, and they cost a quarter
/// of the right-hand side of the glass to say something the finger
/// already knew. A thumb in the look area is *pointing at the
/// crosshair* -- that is what the look area is -- so what it does to
/// the block under the crosshair does not need naming somewhere else.
///
/// The three answers are the three a mouse gives, and the split is by
/// time and distance rather than by place:
///
/// * moved  -> the camera turned, and nothing was touched;
/// * short  -> the right button: put a block down, open a chest;
/// * held   -> the left button, down and staying down: mine, or hit.
///
/// **The block is the one under the crosshair, never the one under the
/// finger.** In a perspective view a finger does not point at a block,
/// it points at a direction, and "tap the cube you want" is a stream of
/// near misses. Every point in the look area does the same thing.
/// How long a finger must rest before it is mining rather than placing.
///
/// **Long enough not to fire on a tap, short enough not to feel stuck.**
/// A tap on this screen measures about 90 ms; a player waiting to mine
/// is holding on purpose and does not count the milliseconds. A quarter
/// of a second sits between the two with room either side.
const HOLD_TO_MINE: std::time::Duration = std::time::Duration::from_millis(250);

/// How far past its own edge a button still answers, as a fraction of
/// the screen's shorter side.
///
/// An eighth of a thumb. It used to be a quarter added to the radius,
/// which came to the same thing while every button was the same size;
/// once a player could make one small, a proportional reach shrank with
/// it and the smallest buttons became the hardest to hit -- exactly
/// backwards.
const REACH: f32 = 0.022;
/// What a finger on a screen the player is *reading* turned out to mean.
///
/// ## Why a screen needs its own answer
///
/// The controls above are for a player looking at the world: every
/// button is held, and a press is a press the moment it lands. A menu
/// is the other thing. Its lists are longer than the panel they are in,
/// and the only way a finger has ever scrolled one is by dragging it --
/// so a touch cannot be turned into a click when it *lands*, because at
/// that moment nobody knows yet whether it is a click.
///
/// That is the bug this exists for. A finger could tap and could never
/// scroll, on every list in the game: the settings, the worlds, the
/// servers. Mapping a press straight through to a mouse button
/// foreclosed the decision before the information arrived.
///
/// So the decision is made at the *end*: a finger that stayed put was a
/// tap, and a finger that travelled was a drag and was never a tap at
/// all. **Never both** -- a scroll that also presses whatever it started
/// on is worse than a list that does not scroll, because the player
/// cannot even see what they did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    /// The finger is down and somewhere; whatever is under it should
    /// light up, and nothing has been decided yet.
    Moved,
    /// A scroll, in the units `Event::MouseWheel` already carries, so
    /// the lists do not need to learn a second way to be scrolled.
    Scrolled(f32),
    /// A press, at the point the finger came **up**, and which of the
    /// three presses a mouse and keyboard can make it was.
    Tapped(Chord),
    /// The finger left without meaning anything: it was a drag, it was
    /// the modifier letting go, or the system took it away.
    Nothing,
}

/// Which press a tap turned out to be.
///
/// ## The hole this fills
///
/// A mouse has three ways to touch a stack of things and a phone had
/// one. Shift-click sends a whole stack across; the right button takes
/// half of one. Neither existed on glass, so moving forty pieces of
/// flint out of a chest was forty taps -- the exact chore the game's
/// own design rule forbids, sitting in the one place a player spends
/// their evenings.
///
/// ## Why a second finger, and why a rest
///
/// **A modifier is a finger already on the glass.** That is what a
/// modifier key *is* -- held down before the thing it modifies -- and
/// making the gesture mean the same thing costs nothing, because a
/// second finger meant nothing here before: `Pointer` used to ignore
/// one outright so that a palm could not steal a drag. Nothing else on
/// a menu is being given up.
///
/// It is the cheap gesture and it goes to the common action, which is
/// the argument `hotbar::HOLD_TO_EAT` already makes in the other
/// direction: unloading a pack into a chest is what a player does for
/// minutes at a time, and splitting a stack is what they do to divide
/// seeds between two fields.
///
/// So the rest -- the same press, held -- is the rarer one. Half a
/// second, which is what Android calls a long press everywhere else on
/// the phone, so it is a length the player's hand already knows.
///
/// ## Why the younger finger is the one that acts
///
/// Because both fingers are down and only one of them is pointing at
/// anything, and the order they landed in is the only thing that tells
/// them apart. The finger that was already there is the modifier; the
/// one that arrived after it is the click. A player who lifts the
/// modifier first has simply changed their mind, and gets nothing --
/// which is better than a click landing wherever their other thumb
/// happened to be resting, and off the panel is where "cancel" lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    /// The left button on its own.
    Plain,
    /// The right button: one finger, rested.
    Secondary,
    /// The left button with the modifier held: two fingers.
    Quick,
}

impl Gesture {
    /// Whether the finger is somewhere worth pointing at.
    ///
    /// **A method rather than a `matches!` at the call site**, because
    /// the call site got it wrong and the interface went dead. A tap is
    /// delivered when the finger comes *up*, so the obvious reading --
    /// "the finger has gone, clear the pointer" -- clears it on the one
    /// event that then sends the click, and every screen in this game
    /// works out what was pressed from where its pointer is. Taps
    /// registered and nothing happened.
    ///
    /// The list of variants that mean "there is a point here" belongs
    /// next to the variants, where changing one makes you look at the
    /// other.
    pub fn carries_a_point(self) -> bool {
        matches!(self, Gesture::Moved | Gesture::Tapped(_))
    }

// **There is no `is_click` any more, and its absence is the point.**
// It answered "was that a press", which used to be the whole question:
// a tap became the left mouse button and there was nothing else it
// could have been. A tap carries a [`Chord`] now, and what a chord is
// worth depends on which screen is up -- the two that hold stacks of
// things use all three, and a menu has no use for a right button at
// all. A caller that only asked "was it a press" would be a caller
// throwing that away silently, so the match is left in the open at the
// one place that knows which screen is up: the touch arm in `lib.rs`.
}

/// One finger on a screen that is being read.
#[derive(Debug, Clone, Copy)]
struct OnGlass {
    id: TouchId,
    /// Where it went down, which is what "has it travelled" is measured
    /// against.
    origin: (f32, f32),
    /// Where it was when the last whole line of scroll was handed out.
    scrolled_to: f32,
    /// When it went down: a rest is a press that outlasts
    /// [`HOLD_FOR_SECONDARY`].
    down_at: std::time::Instant,
    /// Whether it has travelled far enough to stop being a tap.
    ///
    /// One way only. A finger that has become a drag cannot go back to
    /// being a tap by wandering home again -- the player has already
    /// scrolled the list, and pressing whatever ends up under their
    /// thumb is exactly the surprise this whole type exists to prevent.
    dragging: bool,
    /// Whether its part in the gesture is already over.
    ///
    /// Set on the finger left behind when the other one produced the
    /// tap, so that a two-finger press is one click and not two. The
    /// same trick `hotbar::Gestures` plays for the same reason.
    spent: bool,
}

/// The fingers on a screen that is being read.
///
/// Deliberately two, and no more. One is the pointer and the other is
/// the modifier it is held with; a third has nothing left to mean, and
/// deciding what three simultaneous drags are worth is a question with
/// no good answer and no player asking it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pointer {
    /// The finger that arrived first. It owns the scrolling, and while
    /// a second one is on the glass it is the modifier.
    first: Option<OnGlass>,
    /// The one that arrived while the first was there.
    second: Option<OnGlass>,
}

/// How far a finger may wander and still have been a tap.
///
/// A fraction of the screen's shorter side, for the reason the module
/// header gives: in pixels this is four times bigger on one phone than
/// on another, and it has to be the same *distance* on both. A finger
/// pressing firmly rolls a millimetre or two, which on the screen this
/// was cut for is about twenty pixels.
const TAP_SLOP: f32 = 0.02;

/// How far the finger drags for one line of scroll.
///
/// A list row, near enough. Larger and the list crawls behind the
/// thumb; smaller and it outruns it -- and a list that moves further
/// than the finger did is one the player stops trusting to stay where
/// they put it.
const SCROLL_LINE: f32 = 0.055;

/// How long a finger must rest before its lift is the *other* button.
///
/// Half a second, which is what Android has called a long press since
/// there were Androids: `ViewConfiguration`'s own default. A number the
/// player's hand already knows is worth more than a better number it
/// does not, and on a screen full of small squares the cost of getting
/// it wrong is a stack split when a stack was meant to move.
///
/// Long enough, too, that a deliberate but unhurried tap is still a
/// tap: the slow end of a real tap on this screen measures about 200 ms.
const HOLD_FOR_SECONDARY: std::time::Duration = std::time::Duration::from_millis(500);

impl OnGlass {
    fn new(id: TouchId, at: (f32, f32), now: std::time::Instant) -> Self {
        Self {
            id,
            origin: at,
            scrolled_to: at.1,
            down_at: now,
            dragging: false,
            spent: false,
        }
    }

    /// Whether this finger has travelled far enough to have stopped
    /// being a tap, and remembers the answer.
    fn note_travel(&mut self, x: f32, y: f32, short: f32) {
        if !self.dragging
            && (x - self.origin.0).hypot(y - self.origin.1) > TAP_SLOP * short
        {
            self.dragging = true;
        }
    }
}

impl Pointer {
    /// Reads one touch event.
    ///
    /// `size` is passed each time rather than stored because a phone
    /// can rotate between two events of the same gesture, and a
    /// threshold measured against the old screen is a threshold
    /// measured against nothing.
    ///
    /// `now` likewise arrives from the caller: whether a press was a
    /// rest is a rule about elapsed time, and a rule about time that
    /// can only be tested by sleeping is a rule that gets tested once.
    pub fn handle(
        &mut self,
        size: Size,
        id: TouchId,
        phase: TouchPhase,
        x: f32,
        y: f32,
        now: std::time::Instant,
    ) -> Gesture {
        let short = {
            let size = size.non_zero();
            (size.width.min(size.height)) as f32
        };
        match phase {
            TouchPhase::Started => {
                if self.first.is_none() {
                    self.first = Some(OnGlass::new(id, (x, y), now));
                } else if self.second.is_none()
                    // **Not once the first has become a drag.** A finger
                    // landing half way through a scroll is a palm, not a
                    // modifier, and letting it become one would turn the
                    // end of every drag into a press.
                    && self.first.is_some_and(|f| f.id != id && !f.dragging)
                {
                    self.second = Some(OnGlass::new(id, (x, y), now));
                }
                Gesture::Moved
            }
            TouchPhase::Moved => {
                // The modifier is followed but never scrolls: two
                // fingers each handing out lines would move a list twice
                // as far as either of them travelled.
                if let Some(finger) = self.second.as_mut().filter(|f| f.id == id) {
                    finger.note_travel(x, y, short);
                    return Gesture::Moved;
                }
                let Some(finger) = self.first.as_mut().filter(|f| f.id == id) else {
                    return Gesture::Moved;
                };
                finger.note_travel(x, y, short);
                if !finger.dragging {
                    return Gesture::Moved;
                }
                // Whole lines only, with the remainder left on the
                // finger: handing out a fraction of a line every event
                // and rounding it away is a list that refuses to move
                // however far it is dragged.
                let travelled = y - finger.scrolled_to;
                let lines = (travelled / (SCROLL_LINE * short)).trunc();
                if lines == 0.0 {
                    return Gesture::Moved;
                }
                finger.scrolled_to += lines * SCROLL_LINE * short;
                // **The content follows the finger**, the way a sheet of
                // paper does: dragging up moves the list up, which shows
                // what was below it. `MouseWheel` counts a scroll
                // towards the end of a list as negative -- see the
                // handler in `lib.rs` -- and a finger moving up is a
                // decreasing y, so the sign already agrees and must not
                // be "corrected".
                Gesture::Scrolled(lines)
            }
            TouchPhase::Ended => self.lift(id, now),
            TouchPhase::Cancelled => {
                // The system taking a finger away is not a click, even
                // if it never moved: a notification pulled down over the
                // screen must not press the button underneath it. And
                // the finger left behind is spent with it -- one of the
                // two was taken, so whatever the pair was going to mean
                // is no longer available to mean it.
                self.forget(id);
                Gesture::Nothing
            }
        }
    }

    /// Lets go of a finger, and says what it did.
    fn lift(&mut self, id: TouchId, now: std::time::Instant) -> Gesture {
        // The younger finger first: while two are down it is the one
        // that is pointing at something. See [`Chord`].
        if let Some(finger) = self.second.filter(|f| f.id == id) {
            self.second = None;
            let modifier = self.first;
            if finger.spent
                || finger.dragging
                || modifier.is_some_and(|f| f.dragging)
            {
                return Gesture::Nothing;
            }
            // The hand that is left cannot also be a click: two fingers
            // pressed once, not twice.
            if let Some(first) = self.first.as_mut() {
                first.spent = true;
            }
            return Gesture::Tapped(match modifier {
                Some(_) => Chord::Quick,
                // Its partner has already gone, so there was nothing
                // holding it: an ordinary press, decided by how long it
                // stayed like any other.
                None => Self::alone(finger, now),
            });
        }

        let Some(finger) = self.first.filter(|f| f.id == id) else {
            return Gesture::Nothing;
        };
        self.first = None;
        // The elder finger going while a younger one is still down is
        // the modifier letting go, not a press. The younger one is
        // promoted: it is the only finger on the glass now, and it is
        // still on whatever it was pointing at, so it can still be a
        // click of its own.
        if let Some(younger) = self.second.take() {
            self.first = Some(younger);
            return Gesture::Nothing;
        }
        if finger.spent || finger.dragging {
            return Gesture::Nothing;
        }
        Gesture::Tapped(Self::alone(finger, now))
    }

    /// What a single finger's lift was, by how long it stayed.
    fn alone(finger: OnGlass, now: std::time::Instant) -> Chord {
        if now.duration_since(finger.down_at) >= HOLD_FOR_SECONDARY {
            Chord::Secondary
        } else {
            Chord::Plain
        }
    }

    /// Drops one finger and spends the other.
    fn forget(&mut self, id: TouchId) {
        if self.first.is_some_and(|f| f.id == id) {
            self.first = self.second.take();
        } else if self.second.is_some_and(|f| f.id == id) {
            self.second = None;
        } else {
            return;
        }
        if let Some(left) = self.first.as_mut() {
            left.spent = true;
        }
    }

    /// Forgets whatever was in progress.
    ///
    /// For the moments the screen underneath changes out from under the
    /// finger -- a menu opening, a world loading -- where carrying the
    /// gesture over would land a tap on a screen it was never aimed at.
    pub fn release(&mut self) {
        self.first = None;
        self.second = None;
    }
}

/// Everything the fingers are currently doing.
#[derive(Default)]
pub struct Touch {
    layout: Layout,
    /// What INTERFACE SIZE was when the layout was last worked out.
    /// See [`Touch::resize`].
    ui_scale: f32,
    /// The finger on the movement stick.
    stick: Option<Finger>,
    /// The finger turning the camera.
    look: Option<Finger>,
    /// Which button each held finger is on.
    ///
    /// A list rather than a set of flags because a finger has to be
    /// tracked back to the button it went down on: a thumb that slides
    /// off the jump button must stop jumping, and the only way to know
    /// which button to release is to remember which one it took.
    held: Vec<(TouchId, Slot)>,
    /// Look movement since the last frame drained it.
    look_delta: (f32, f32),
    /// A short tap in the look area, waiting to be collected.
    ///
    /// A flag rather than a queue: at most one finger is in the look
    /// area, and two taps between two frames are two blocks the player
    /// could not have aimed separately anyway.
    place: bool,
    /// Whether the wheel's members are out.
    ///
    /// **Here and not in the layout's own hands**, even though the
    /// layout carries a copy: the layout is worked out afresh from a
    /// screen size and an arrangement, and a value that survives that
    /// has to be owned by something that survives it too. The copy in
    /// the layout is what the drawing and the hit-testing read, and it
    /// is written from here.
    wheel_open: bool,
}



impl Touch {
    /// Tells the controls how big the screen is.
    ///
    /// Cheap, and called every frame rather than on resize only: a
    /// phone rotating is a resize the game may not have been told about
    /// yet, and laying out four circles is arithmetic.
    pub fn resize(
        &mut self,
        size: Size,
        arrangement: crate::settings::TouchLayout,
        ui_scale: f32,
    ) {
        // The scale is compared too, because the bar the controls are
        // kept clear of grows with it -- see `lift_clear_of_the_bar`.
        // Left out, a player moving INTERFACE SIZE would widen the
        // hotbar under buttons that never moved.
        let scale_changed = self.ui_scale != ui_scale;
        if self.layout.size != size
            || !self.layout.arrangement.same_as(&arrangement)
            || scale_changed
            || self.layout.wheel_open != self.wheel_open
        {
            self.ui_scale = ui_scale;
            self.layout = Layout::for_size(size, arrangement, ui_scale, self.wheel_open);
        }
    }

    /// Opens or closes the wheel, and re-lays the controls at once.
    ///
    /// **At once, and not at the next `resize`.** The layout is what
    /// both the drawing and the hit-testing read, and the touch that
    /// opens the wheel is followed by touches -- and by a frame --
    /// before anything calls `resize` again. Leaving it until then
    /// would mean one frame in which the members are out and cannot be
    /// pressed, which reads as a wheel that ignores the first tap.
    fn set_wheel(&mut self, open: bool) {
        if self.wheel_open == open {
            return;
        }
        self.wheel_open = open;
        self.layout = Layout::for_size(
            self.layout.size,
            self.layout.arrangement,
            self.ui_scale,
            open,
        );
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Whether a button is currently held down.
    pub fn is_held(&self, slot: Slot) -> bool {
        self.held.iter().any(|(_, held)| *held == slot)
    }

    /// Every button with a thumb on it.
    ///
    /// For the one thing that is a level rather than an edge -- mining
    /// -- where the caller has to ask "is any button that means *this*
    /// held", and only the caller knows what a button means.
    pub fn held_slots(&self) -> impl Iterator<Item = Slot> + '_ {
        self.held.iter().map(|(_, slot)| *slot)
    }

    /// Feeds one touch in, and says what it did to the buttons.
    /// `now` is the moment this touch happened, and it is passed in
    /// rather than read here so that the rule deciding place-or-mine is
    /// a pure function of it -- see [`Hand`] and the tests below, which
    /// hand it invented times.
    pub fn handle(
        &mut self,
        id: TouchId,
        phase: TouchPhase,
        x: f32,
        y: f32,
        now: std::time::Instant,
    ) -> Hit {
        match phase {
            TouchPhase::Started => self.down(id, x, y, now),
            TouchPhase::Moved => self.moved(id, x, y),
            // A cancelled touch is a finger the system took away -- a
            // notification pulled down mid-stride. Treated exactly like
            // a lift, because the alternative is a player who comes
            // back to the game still walking, or still mining.
            TouchPhase::Ended | TouchPhase::Cancelled => self.up(id),
        }
    }

    fn down(&mut self, id: TouchId, x: f32, y: f32, now: std::time::Instant) -> Hit {
        // Buttons first: they sit inside the look area, and a thumb on
        // the mine button must mine rather than turn the camera.
        if let Some(slot) = self.layout.button_at(x, y) {
            self.held.push((id, slot));
            // The hub opens and closes on the *press* rather than on
            // the lift, because everything else on this glass happens
            // when the thumb lands and a wheel that waited would be the
            // one control in the game that feels slow. Its members are
            // dismissed on the lift instead -- see `up` -- so that a
            // member stays drawn, and drawn as pressed, for as long as
            // the finger is on it.
            if self.layout.hub() == Some(slot) {
                let open = !self.wheel_open;
                self.set_wheel(open);
            }
            return Hit::Pressed(slot);
        }
        // **A touch anywhere else puts the wheel away and does nothing
        // else.** Dismissing is what a finger outside an open menu
        // means on every phone there is, and letting the same touch
        // also walk, look or place would make the dismissal cost the
        // player a block in a wall they did not mean to build.
        if self.wheel_open {
            self.set_wheel(false);
            return Hit::Nothing;
        }
        if self.layout.is_look_area(x, y) {
            if self.look.is_none() {
                self.look = Some(Finger {
                    id,
                    origin: (x, y),
                    at: (x, y),
                    down_at: Some(now),
                    dragged: false,
                    mining: false,
                });
            }
            return Hit::Nothing;
        }
        // Everything else on the left is the stick, centred wherever the
        // thumb landed. One at a time: a second finger on the left is a
        // stray palm, not a second player.
        if self.stick.is_none() {
            self.stick = Some(Finger {
                id,
                origin: (x, y),
                at: (x, y),
                // The stick is not a hand: it never places or mines, so
                // it has no clock and never becomes a drag.
                down_at: None,
                dragged: false,
                mining: false,
            });
        }
        Hit::Nothing
    }

    fn moved(&mut self, id: TouchId, x: f32, y: f32) -> Hit {
        let short = self.layout.size.non_zero().shorter() as f32;
        if let Some(finger) = self.stick.as_mut().filter(|f| f.id == id) {
            finger.at = (x, y);
            return Hit::Nothing;
        }
        if let Some(finger) = self.look.as_mut().filter(|f| f.id == id) {
            // Accumulated rather than assigned: several moves can
            // arrive between two frames, and the camera wants all of
            // them, not the last one.
            self.look_delta.0 += x - finger.at.0;
            self.look_delta.1 += y - finger.at.1;
            finger.at = (x, y);
            // Far enough from where it landed and it is a camera drag,
            // which is neither a place nor a mine. **Not cancelled once
            // mining has started**: a player breaking a block is
            // allowed to look around while they do it, exactly as a
            // mouse held down is.
            if !finger.mining
                && (x - finger.origin.0).hypot(y - finger.origin.1) > TAP_SLOP * short
            {
                finger.dragged = true;
            }
            return Hit::Nothing;
        }
        // A finger that went down on a button and slid off it stops
        // pressing it. Sliding *onto* a button does not press it: that
        // is how a thumb reaching for the stick sets off the inventory.
        //
        // **And it has to say so.** This used to drop the slot out of
        // `held` and answer `Nothing`, which is the same shape of bug
        // the whole `Hit::Released` variant exists to prevent, one
        // level down: the button had stopped being held here and the
        // key it emulates was never released up there. A thumb that
        // pressed JUMP and slid off towards the stick left SPACE down
        // for the rest of the session, and the player bounced across
        // the world until they pressed the button again and let go of
        // it properly.
        if let Some(index) = self.held.iter().position(|(held, _)| *held == id) {
            if self.layout.button_at(x, y).is_none() {
                let (_, slot) = self.held.swap_remove(index);
                return Hit::Released(slot);
            }
        }
        Hit::Nothing
    }

    fn up(&mut self, id: TouchId) -> Hit {
        if self.stick.is_some_and(|f| f.id == id) {
            self.stick = None;
        }
        if let Some(finger) = self.look.filter(|f| f.id == id) {
            self.look = None;
            // What the hand did, decided at the moment it lifts. A
            // finger that mined has already done its work, and one that
            // dragged turned the camera and nothing else.
            self.place = !finger.mining && !finger.dragged;
        }
        // The slot this finger was on, before it is forgotten. The
        // caller needs it to release the key the button emulates; a
        // release that never arrives is a key stuck down.
        let released = self
            .held
            .iter()
            .find(|(held, _)| *held == id)
            .map(|(_, slot)| *slot);
        self.held.retain(|(held, _)| *held != id);
        // A member has been chosen, so the wheel has done its job. Closed
        // on the lift rather than on the press for the reason `down`
        // gives: the button has to still be there, and still be drawn
        // as held, while the thumb is on it.
        if released.is_some_and(|slot| self.layout.is_wheel_member(slot)) {
            self.set_wheel(false);
        }
        match released {
            Some(slot) => Hit::Released(slot),
            None => Hit::Nothing,
        }
    }

    /// Whether the hand in the look area is breaking something.
    ///
    /// ## Why the hands are a gesture and not two buttons
    ///
    /// They were two buttons, `MINE` and `PLACE`, and they cost a
    /// quarter of the right-hand side of the glass to say something the
    /// finger already knew. A thumb in the look area is *pointing at
    /// the crosshair* -- that is what the look area is -- so what it
    /// does to the block under the crosshair does not need naming
    /// somewhere else.
    ///
    /// Three answers, split by time and distance rather than by place:
    /// moved turns the camera and touches nothing; a short tap is the
    /// right button; a rested finger is the left button, down and
    /// staying down. **The block is the one under the crosshair, never
    /// the one under the finger** -- in a perspective view a finger
    /// points at a direction, and "tap the cube you want" is a stream
    /// of near misses.
    ///
    /// **Asked every frame, and that is not tidiness.** A finger that
    /// rests without moving produces no touch events at all, so the
    /// moment it stops being a tap and becomes a hold arrives while the
    /// platform has nothing to say. Waiting for the next event would
    /// mean a player could hold still for ever and never start mining
    /// -- which is exactly what holding still means.
    pub fn is_mining(&mut self, now: std::time::Instant) -> bool {
        let Some(finger) = self.look.as_mut() else {
            return false;
        };
        if finger.dragged {
            return false;
        }
        // Latched once it starts, so that looking around while breaking
        // a block does not put the pick down. `moved` stops widening
        // `dragged` for the same reason.
        finger.mining = finger.mining
            || finger.down_at.is_some_and(|at| now.duration_since(at) >= HOLD_TO_MINE);
        finger.mining
    }

    /// Whether the hand asked to place, once.
    ///
    /// An edge where mining is a level, because that is what the two
    /// are: the right mouse button is clicked and the left one is held.
    /// Taken rather than read, so one tap puts down one block however
    /// many times the frame loop asks.
    pub fn take_place(&mut self) -> bool {
        std::mem::take(&mut self.place)
    }

    /// Every finger forgotten.
    ///
    /// Called when the game stops being the thing on screen -- a pause
    /// menu, the activity going away. Touch-up does not arrive for a
    /// finger that was on the glass when the window went, so without
    /// this the player comes back still walking.
    pub fn release_all(&mut self) {
        // A finger that was mining when the world stopped owning the
        // glass has to let go, or the player comes back still breaking
        // the block they were looking at. The same argument the buttons
        // make just below.
        let was_mining = self.look.is_some_and(|f| f.mining);
        self.stick = None;
        self.look = None;
        self.held.clear();
        // The wheel goes away with the fingers. A player who opens
        // their pack and comes back to a wheel they left open two
        // minutes ago has been handed a control they did not choose --
        // and worse, one sitting where they are about to aim.
        self.set_wheel(false);
        self.look_delta = (0.0, 0.0);
        // Nothing left half-done: the level goes with the finger, and a
        // tap nobody collected is not owed to a screen the player has
        // since opened.
        let _ = was_mining;
        self.place = false;
    }

    /// How far the stick is pushed, as a direction and a strength.
    ///
    /// `(0, 0)` when no thumb is on it. X is right, Y is *forward*,
    /// which is up the screen and therefore the negative pixel
    /// direction -- the flip happens here so nothing above has to
    /// remember it.
    pub fn stick(&self) -> (f32, f32) {
        let Some(finger) = self.stick else {
            return (0.0, 0.0);
        };
        let dx = finger.at.0 - finger.origin.0;
        let dy = finger.at.1 - finger.origin.1;
        let radius = self.layout.stick.radius().max(1.0);
        let (mut x, mut y) = (dx / radius, -dy / radius);

        // Clamped as a vector, not per-axis. Dividing each axis
        // separately lets a diagonal push reach 1.41 times the length
        // of a straight one, which is the classic bug that makes
        // running diagonally the fastest way across a world.
        let mut length = (x * x + y * y).sqrt();
        if length > 1.0 {
            x /= length;
            y /= length;
            length = 1.0;
        }

        // A dead zone, because a thumb resting on the glass is not a
        // thumb asking to walk. Rescaled rather than cut off: the
        // strength runs from nothing at the edge of the zone to full at
        // the rim, so the first movement past it starts from a
        // standstill instead of jumping straight to a fifth of walking
        // speed.
        const DEAD_ZONE: f32 = 0.2;
        if length <= DEAD_ZONE {
            return (0.0, 0.0);
        }
        let strength = (length - DEAD_ZONE) / (1.0 - DEAD_ZONE);
        let scale = strength / length;
        (x * scale, y * scale)
    }

    /// The look movement since this was last called, and clears it.
    ///
    /// In the same units a mouse reports -- pixels -- so the player's
    /// existing sensitivity setting means the same thing on both.
    pub fn take_look_delta(&mut self) -> (f32, f32) {
        std::mem::replace(&mut self.look_delta, (0.0, 0.0))
    }
}

#[cfg(test)]
mod tests {
    /// The phone this was cut for, held sideways.
    fn phone() -> Size {
        Size { width: 2712, height: 1220 }
    }

    /// Walks a finger through a gesture and answers what came out.
    fn gesture(points: &[(f32, f32)]) -> Vec<Gesture> {
        let mut pointer = Pointer::default();
        let mut out = Vec::new();
        for (index, &(x, y)) in points.iter().enumerate() {
            let phase = match index {
                0 => TouchPhase::Started,
                n if n == points.len() - 1 => TouchPhase::Ended,
                _ => TouchPhase::Moved,
            };
            out.push(pointer.handle(phone(), 1, phase, x, y, moment(index as u64 * 20)));
        }
        out
    }

    /// No thumb button is drawn over the hotbar, at any interface size.
    ///
    /// **The first version of this test measured the wrong bar and
    /// passed while the bug was on screen.** It compared the buttons
    /// against `hotbar::RIGHT` as authored -- and the bar a player sees
    /// is that one multiplied by INTERFACE SIZE, because the whole HUD
    /// is grown about the bottom edge and the thumb controls
    /// deliberately are not. At the phone's own default of 1.5 the bar
    /// reached 120 pixels further than the test believed, and the tenth
    /// slot was drawn under the sneak button.
    ///
    /// So the sweep is over the range of the setting, not one value of
    /// it: `ClientSettings::sanitize` clamps INTERFACE SIZE to 0.5..4,
    /// and a layout that is only right in the middle of that is a
    /// layout that is wrong for somebody.
    #[test]
    fn no_thumb_button_is_drawn_over_the_hotbar_at_any_interface_size() {
        use crate::ui::hotbar;

        let size = phone();
        let (w, h) = (size.width as f32, size.height as f32);
        let per_unit = h / 2.0;

        for (scale, wheel_open) in [0.5f32, 1.0, 1.5, 2.0, 3.0, 4.0]
            .into_iter()
            .flat_map(|scale| [(scale, false), (scale, true)])
        {
            let layout =
                Layout::for_size(size, crate::settings::TouchLayout::default(), scale, wheel_open);
            // The bar as drawn: grown about the bottom edge.
            let grown = |y: f32| -1.0 + (y + 1.0) * scale;
            let half_width = hotbar::RIGHT * scale * per_unit;
            let bar_left = w / 2.0 - half_width;
            let bar_right = w / 2.0 + half_width;
            let bar_top = (1.0 - grown(hotbar::TOP)) * per_unit;

            for slot in 0..crate::settings::TouchLayout::BUTTONS {
                let button = &layout.buttons[slot];
                if !button.shown {
                    continue;
                }
                let (left, right) = (button.centre.0 - button.half.0, button.centre.0 + button.half.0);
                let bottom = button.centre.1 + button.half.1;
                assert!(
                    right <= bar_left || left >= bar_right || bottom <= bar_top,
                    "at interface size {scale} (wheel open: {wheel_open}) button {slot} ({:?}) covers                      the hotbar: it spans {left}..{right} and down to {bottom}, against a bar of                      {bar_left}..{bar_right} with its top at {bar_top}",
                    button.emits,
                );
            }
        }
    }

    /// No two controls are ever drawn through one another.
    ///
    /// **This is the check that was missing, and a player found what
    /// it was missing.** On a phone at INTERFACE SIZE 1.45 the three
    /// buttons along the top edge were drawn overlapping -- CHAT's
    /// frame across MENU's, F3's across CHAT's, and the word CHAT cut
    /// off by the frame that was standing on it. Every layout test in
    /// this file was written against one interface size, or against
    /// one arrangement, and none of them ever asked the plainest
    /// question there is about a set of boxes.
    ///
    /// So it asks it across the whole range the setting allows
    /// (`ClientSettings::sanitize` clamps INTERFACE SIZE to 0.5..4),
    /// over several screen shapes, and with the wheel both shut and
    /// open -- because the wheel is the arrangement's tightest moment
    /// and the one the player is looking at while they choose.
    ///
    /// Boxes rather than hit areas: the reach deliberately overlaps
    /// (see `button_at`), and what must not overlap is what is drawn.
    #[test]
    fn no_two_controls_are_drawn_on_top_of_one_another_at_any_interface_size() {
        for (w, h) in [(2712, 1220), (2400, 1080), (1280, 720), (2960, 1440), (1080, 2400)] {
            for scale in [0.5f32, 1.0, 1.25, 1.45, 1.5, 1.65, 2.0, 3.0, 4.0] {
                for wheel_open in [false, true] {
                    let layout = Layout::for_size(
                        Size::new(w, h),
                        crate::settings::TouchLayout::default(),
                        scale,
                        wheel_open,
                    );
                    // The stick is a control too: a button drawn over
                    // the ring is a button a left thumb sets off.
                    let mut drawn: Vec<(String, Placed)> = vec![("stick".to_string(), layout.stick)];
                    for slot in 0..crate::settings::TouchLayout::BUTTONS {
                        drawn.push((format!("{:?}", layout.buttons[slot].emits), layout.buttons[slot]));
                    }
                    drawn.retain(|(_, placed)| placed.shown);

                    // **Measured to the drawn edge, not to the box,
                    // and with air demanded rather than mere absence of
                    // overlap.** The box is what is hit-tested; the
                    // frame is drawn a stroke and a half outside it, on
                    // every side -- see `hud::frame_overhang`, and see
                    // the picture the player sent, in which two buttons
                    // 33 px apart were six pixels of air from touching
                    // and read as one bar.
                    let short = w.min(h) as f32;
                    for (index, (name, one)) in drawn.iter().enumerate() {
                        for (other, two) in &drawn[index + 1..] {
                            assert!(
                                !crowds(one, two, short),
                                "at {w}x{h}, interface size {scale}, wheel open {wheel_open}: \
                                 {name} at {:?} and {other} at {:?} are drawn into one another",
                                one.centre,
                                two.centre,
                            );
                        }
                    }
                }
            }
        }
    }

    /// ...and the stick is left where the hand rests.
    ///
    /// The rule that lifts a control off the bar has to be a rule about
    /// *overlapping* it, not about being near the bottom. The movement
    /// stick sits in the bottom-left corner, well outside a centred
    /// bar, and a rule that pushed everything up would take the corner
    /// a left thumb lives in and give back nothing.
    #[test]
    fn the_stick_is_not_moved_by_a_bar_it_does_not_touch() {
        let size = phone();
        let plain = Layout::for_size(size, crate::settings::TouchLayout::default(), 1.0, false);
        let grown = Layout::for_size(size, crate::settings::TouchLayout::default(), 3.0, false);
        assert_eq!(
            plain.stick.centre, grown.stick.centre,
            "the stick moved when the hotbar grew, and it never overlapped it",
        );
    }

    /// A control dragged to a point is written down as that point.
    ///
    /// **The one thing the arrangement editor cannot get wrong.** It
    /// moves a control with a finger and writes an arrangement; the game
    /// then draws the control from that arrangement. If the two
    /// conversions disagree by so much as a pixel, the control jumps out
    /// from under the finger the moment it is let go -- and a player
    /// aiming a button at their thumb would be chasing it.
    ///
    /// Swept over the whole glass, corners included, because the corner
    /// a placement is measured from changes across the middle and the
    /// seam is exactly where an inverse stops being one.
    #[test]
    fn a_control_dragged_to_a_point_is_written_down_as_that_point() {
        let size = phone();
        let (w, h) = (size.width as f32, size.height as f32);
        let was = crate::settings::TouchLayout::default().buttons[0];

        for across in 0..=12 {
            for down in 0..=12 {
                let wanted = (
                    w * across as f32 / 12.0,
                    h * down as f32 / 12.0,
                );
                let written = placement_for(wanted, size, was);
                let landed = place_one(&written, size).centre;
                // Where it *can* go: a control cannot hang off the
                // edge, so the honest target is the clamped point.
                let (half_w, half_h) = written.half();
                let short = w.min(h);
                let expected = (
                    keep_inside(wanted.0, w, half_w * short),
                    keep_inside(wanted.1, h, half_h * short),
                );
                assert!(
                    (landed.0 - expected.0).abs() < 0.01
                        && (landed.1 - expected.1).abs() < 0.01,
                    "dragged to {wanted:?}, written as {written:?}, landed at {landed:?}                      instead of {expected:?}",
                );
            }
        }
    }

    /// A control carried across the middle changes which corner it is
    /// measured from.
    ///
    /// That is the whole reason a corner is part of a placement: a
    /// button on the right is two thumbs in *from the right*, so it is
    /// still under the right thumb on a screen of another shape. A
    /// button moved to the other thumb has to be re-cornered or it
    /// drifts across the glass the first time the phone is turned.
    #[test]
    fn a_button_carried_to_the_other_thumb_is_measured_from_that_corner() {
        use crate::settings::Corner;
        let size = phone();
        let (w, h) = (size.width as f32, size.height as f32);
        let was = crate::settings::TouchLayout::default().buttons[0];

        let bottom_left = placement_for((w * 0.1, h * 0.9), size, was);
        assert_eq!(bottom_left.corner, Corner::BottomLeft);
        let bottom_right = placement_for((w * 0.9, h * 0.9), size, was);
        assert_eq!(bottom_right.corner, Corner::BottomRight);
        let top_left = placement_for((w * 0.1, h * 0.1), size, was);
        assert_eq!(top_left.corner, Corner::TopLeft);

        // ...and the two bottom ones are the same distance in from
        // their own edges, which is what "the same place, mirrored"
        // means and what makes the arrangement survive a rotation.
        assert!(
            (bottom_left.inset.0 - bottom_right.inset.0).abs() < 0.01,
            "{:?} vs {:?}",
            bottom_left.inset,
            bottom_right.inset,
        );
    }

    /// Every thumb button is somewhere a thumb can get to.
    ///
    /// A phone held sideways is held by its two bottom corners, and the
    /// thumbs swing from there. A control in the top half is on screen
    /// and unreachable, which is the same as missing -- and it was:
    /// with the buttons stacked four high the top of the block stood at
    /// 86% of the way up the glass, and the pack was one of the two up
    /// there.
    ///
    /// **Only the ones pressed while moving.** The rule used to be
    /// "every button", and that was right when every button was one a
    /// player needs mid-stride. It is wrong now, and wrong in a way
    /// worth stating: a button that *stops the game* -- the pause menu,
    /// the chat box -- must not be where the playing thumb rests, or it
    /// gets opened by a mis-aimed swing. Those are deliberately out of
    /// reach; reaching for them means letting go, which is the right
    /// cost for an action that interrupts the game anyway.
    ///
    /// So the test splits by what the button does. Jump and the pack
    /// have to be under the thumb. Escape, chat and the debug readout
    /// have to be clear of both bottom corners, where the two thumbs
    /// live.
    #[test]
    fn the_buttons_pressed_while_moving_are_the_ones_under_the_thumb() {
        use crate::platform::Key;
        use crate::settings::Emits;

        let size = phone();
        let (w, h) = (size.width as f32, size.height as f32);

        // **Both states, and both are needed.** The wheel members are
        // three of the buttons that must not be under a playing thumb,
        // and they are somewhere else entirely while it is open; the
        // buttons that must be *under* the thumb are on the glass while
        // it is shut, and an open wheel can take one of them off (see
        // `yield_to_the_wheel`), where an unshown button is skipped.
        // Either half alone leaves one of the two rules unchecked.
        for wheel_open in [false, true] {
        let layout =
            Layout::for_size(size, crate::settings::TouchLayout::default(), 1.5, wheel_open);
        for slot in 0..crate::settings::TouchLayout::BUTTONS {
            let button = &layout.buttons[slot];
            if !button.shown {
                continue;
            }
            // Pixel coordinates run down the screen, so a small y is
            // high up and far from the hand.
            let top = button.centre.1 - button.half.1;
            let bottom = button.centre.1 + button.half.1;
            match button.emits {
                // Wanted mid-stride: within a thumb's swing of the
                // bottom. The line is generous on purpose -- it is here
                // to fail when a column grows tall again, not to pin the
                // arrangement in place.
                // The modifier is in this half of the split and not
                // the other, and that is the whole argument for where
                // it sits: shift is held *while* placing, dropping or
                // running, so a shift out of reach of the thumb that
                // holds it is a shift nobody can use. It is the one
                // mid-stride control on the left, above the stick --
                // see the spot in `settings::TouchLayout::default`.
                Emits::Key(Key::Space) | Emits::Key(Key::KeyI) | Emits::Key(Key::ShiftLeft) => assert!(
                    top >= h * 0.45,
                    "{:?} reaches {top} up a {h}-tall screen and is meant to be under a thumb",
                    button.emits,
                ),
                // Everything else takes the player out of the world, so
                // it must be clear of both bottom corners.
                _ => {
                    let in_a_bottom_corner = bottom > h * 0.55
                        && (button.centre.0 < w * 0.25 || button.centre.0 > w * 0.75);
                    assert!(
                        !in_a_bottom_corner,
                        "{:?} sits at {:?} -- in a bottom corner, where a thumb is playing",
                        button.emits,
                        button.centre,
                    );
                }
            }
        }
        }
    }

    /// A thumb between two buttons gets the one it is nearer to.
    ///
    /// The reach that makes a button easier to hit also makes buttons
    /// that are close together overlap each other's claims. This used
    /// to be resolved by slot number, and mine was the slot below
    /// place -- so the whole band between them, including the half
    /// sitting against place's own edge, dug a block out instead of
    /// placing one. The block a player destroyed that way was the one
    /// they were standing on.
    ///
    /// **The arrangement is built here rather than taken from the
    /// defaults**, and that is a fix of its own. It used to walk the
    /// shipped layout looking for a pair whose reaches meet, which
    /// worked only for as long as the shipped layout had one: the
    /// buttons were spaced further apart to stop their frames reading
    /// as one bar (see `CLEARANCE`), the reaches stopped meeting, and a
    /// test about what happens when they do had nothing left to look
    /// at. A player can still put two buttons that close, and the rule
    /// still has to hold when they do.
    #[test]
    fn a_thumb_landing_between_two_buttons_presses_the_nearer_one() {
        use crate::settings::{Corner, Emits, Placement};

        let size = phone();
        let short = size.shorter() as f32;
        // Two buttons in a column with a bare gap narrower than twice
        // the reach, so both of them claim the middle of it.
        const SIDE: f32 = 0.18;
        let gap = REACH * 1.2;
        let mut arrangement = crate::settings::TouchLayout::default();
        for button in arrangement.buttons.iter_mut() {
            button.shown = false;
        }
        arrangement.buttons[0] = Placement {
            corner: Corner::TopLeft,
            inset: (0.5, 0.5),
            width: SIDE,
            height: SIDE,
            shown: true,
            emits: Emits::Mine,
            in_wheel: false,
        };
        arrangement.buttons[1] = Placement {
            corner: Corner::TopLeft,
            inset: (0.5, 0.5 + SIDE + gap),
            width: SIDE,
            height: SIDE,
            shown: true,
            emits: Emits::Place,
            in_wheel: false,
        };
        let layout = Layout::for_size(size, arrangement, 1.5, false);

        let (upper, lower) = (0, 1);
        let (above, below) = (&layout.buttons[upper], &layout.buttons[lower]);
        // `above` is the one further up the glass, so its edge is the
        // smaller y.
        let (top, bottom) = (above.centre.1 + above.half.1, below.centre.1 - below.half.1);
        assert!(bottom > top, "the fixture put the two buttons on top of each other");
        assert!(
            bottom - top < 2.0 * REACH * short,
            "the fixture left a gap neither button can reach into, which is not what \
             this is about",
        );
        let x = below.centre.0;

        // A hair on each side of the middle of the gap: the two points
        // are all but the same place, and they must answer with
        // different buttons.
        let middle = (top + bottom) / 2.0;
        assert_eq!(
            layout.button_at(x, middle - 1.0),
            Some(upper),
            "just above the middle of the gap belongs to button {upper}",
        );
        assert_eq!(
            layout.button_at(x, middle + 1.0),
            Some(lower),
            "just below the middle of the gap belongs to button {lower}",
        );

        // And being *on* a button is never overruled by a neighbour
        // reaching across it.
        assert_eq!(layout.button_at(x, above.centre.1), Some(upper));
        assert_eq!(layout.button_at(x, below.centre.1), Some(lower));
    }

    /// Reaching past an edge never reaches past the halfway line.
    ///
    /// The generous hit area exists so a button is easy to hit, not so
    /// that it steals from the button beside it. Whatever the spacing,
    /// every point in the gap belongs to whichever button it is nearer
    /// -- which is the property that makes the reach safe to widen.
    #[test]
    fn no_button_answers_for_a_touch_that_is_nearer_to_another_one() {
        // Open, because that is when there are most of them: the wheel
        // puts three more buttons on an arc round a fourth, which is
        // the tightest the glass ever gets.
        let layout = Layout::for_size(phone(), crate::settings::TouchLayout::default(), 1.5, true);
        for slot in 0..crate::settings::TouchLayout::BUTTONS {
            if !layout.buttons[slot].shown {
                continue;
            }
            let button = &layout.buttons[slot];
            // A grid over the button and well past its edges, so the
            // sweep covers the whole band it can reach into.
            let span = button.half.1 * 3.0;
            for step in 0..=60 {
                let y = button.centre.1 - span + span * 2.0 * step as f32 / 60.0;
                let x = button.centre.0;
                let Some(answer) = layout.button_at(x, y) else {
                    continue;
                };
                let mine = layout.buttons[answer].distance_outside(x, y);
                for other in 0..crate::settings::TouchLayout::BUTTONS {
                    if other == answer || !layout.buttons[other].shown {
                        continue;
                    }
                    let theirs = layout.buttons[other].distance_outside(x, y);
                    assert!(
                        mine <= theirs,
                        "at ({x}, {y}) button {answer} answered at {mine} away                          while button {other} was nearer at {theirs}",
                    );
                }
            }
        }
    }

    /// A finger the game stopped listening to still has to be let go
    /// of.
    ///
    /// The sequence that used to leave a key held down for ever: a
    /// thumb presses the button that opens the inventory, the screen
    /// opens, and *the screen* now owns touches -- so the lift is
    /// delivered to the menu pointer and never reaches here. The slot
    /// stays in `held`, the key it emulates is never released, and the
    /// player comes back from the inventory still mining.
    ///
    /// `release_all` is the way out, and this is what says it works. It
    /// is called wherever the world stops owning the glass.
    #[test]
    fn a_button_is_let_go_of_when_the_game_stops_watching_the_finger() {
        let mut touch = Touch::default();
        touch.resize(Size::new(2400, 1080), crate::settings::TouchLayout::default(), 1.5);
        let (bx, by) = touch.layout.buttons[0].centre;

        touch.handle(1, TouchPhase::Started, bx, by, moment(0));
        assert!(touch.is_held(0));

        // The lift never arrives -- a screen took the finger.
        touch.release_all();
        assert!(!touch.is_held(0));
        assert_eq!(touch.held_slots().count(), 0);
        assert_eq!(touch.stick(), (0.0, 0.0), "the stick was still pushed");
    }

    /// A thumb that lands on a button and lifts reports both edges.
    ///
    /// The reason `Hit` carries a release at all. A button emulates a
    /// key, and the game turns `Pressed` into a key going down; without
    /// the matching `Released` the key stays down for ever and the
    /// player keeps mining, or keeps running, long after their thumb
    /// has left the glass. `up` has to look the slot up *before* it
    /// forgets the finger, which is the part easiest to lose in a
    /// tidy-up.
    #[test]
    fn a_button_reports_the_lift_as_well_as_the_press() {
        let mut touch = Touch::default();
        touch.resize(Size::new(2400, 1080), crate::settings::TouchLayout::default(), 1.5);
        let (bx, by) = touch.layout.buttons[0].centre;

        assert_eq!(touch.handle(1, TouchPhase::Started, bx, by, moment(0)), Hit::Pressed(0));
        assert!(touch.is_held(0));
        assert_eq!(touch.handle(1, TouchPhase::Ended, bx, by, moment(0)), Hit::Released(0));
        assert!(!touch.is_held(0));

        // A finger that never touched a button has nothing to release.
        assert_eq!(touch.handle(2, TouchPhase::Ended, 5.0, 5.0, moment(0)), Hit::Nothing);
    }

    #[test]
    fn a_tap_is_a_click_and_says_where_it_landed() {
        // **The third time a touch change made the interface do
        // nothing, and the mechanism each time was different**: the
        // stick swallowing every touch, then the panels unable to grow,
        // then this -- the pointer cleared on the very event that sent
        // the click, so every screen worked out what was pressed from a
        // pointer that had just been set to nothing.
        //
        // What has to hold is both halves at once. A tap is a press
        // *and* it is somewhere.
        let mut pointer = Pointer::default();
        pointer.handle(phone(), 1, TouchPhase::Started, 900.0, 500.0, moment(0));
        // A little travel, well inside the slop: a real thumb rolls.
        let moved = pointer.handle(phone(), 1, TouchPhase::Moved, 906.0, 496.0, moment(0));
        assert!(moved.carries_a_point(), "the finger is on the glass");
        assert!(!moved.was_a_press(), "nothing has been decided yet");

        let lifted = pointer.handle(phone(), 1, TouchPhase::Ended, 904.0, 498.0, moment(0));
        assert!(lifted.was_a_press(), "a finger that stayed put pressed nothing");
        assert!(
            lifted.carries_a_point(),
            "the click arrived with no pointer under it, which is how a \
             screen drops it in silence",
        );

        // ...and the other way round: a drag ends with no point and no
        // click, so the row the finger started on is neither pressed nor
        // left lit under a thumb that has gone.
        let mut dragged = Pointer::default();
        dragged.handle(phone(), 1, TouchPhase::Started, 900.0, 900.0, moment(0));
        dragged.handle(phone(), 1, TouchPhase::Moved, 900.0, 500.0, moment(0));
        let end = dragged.handle(phone(), 1, TouchPhase::Ended, 900.0, 500.0, moment(0));
        assert!(!end.was_a_press() && !end.carries_a_point(), "got {end:?}");
    }

    #[test]
    fn a_finger_that_stays_put_is_a_tap() {
        // Including one that rolls a little, which every firm press
        // does: a tap that misses because the thumb flexed is a button
        // the player presses twice and blames the game for.
        let out = gesture(&[(600.0, 400.0), (604.0, 397.0), (602.0, 401.0)]);
        assert_eq!(out.last(), Some(&Gesture::Tapped(Chord::Plain)), "got {out:?}");
    }

    #[test]
    fn a_finger_that_travels_scrolls_and_is_never_also_a_tap() {
        // **The whole point.** A scroll that also presses whatever it
        // started on is worse than a list that does not scroll.
        let out = gesture(&[
            (600.0, 800.0),
            (600.0, 700.0),
            (600.0, 600.0),
            (600.0, 500.0),
        ]);
        assert!(
            out.iter().any(|g| matches!(g, Gesture::Scrolled(_))),
            "nothing scrolled: {out:?}",
        );
        assert_eq!(out.last(), Some(&Gesture::Nothing), "the drag also tapped");
    }

    #[test]
    fn the_list_follows_the_finger_rather_than_running_from_it() {
        // Dragging up shows what was below, the way a sheet of paper
        // moves. `MouseWheel` counts that direction as negative.
        let mut pointer = Pointer::default();
        pointer.handle(phone(), 1, TouchPhase::Started, 600.0, 900.0, moment(0));
        let mut total = 0.0;
        for y in [800.0f32, 700.0, 600.0] {
            if let Gesture::Scrolled(lines) = pointer.handle(phone(), 1, TouchPhase::Moved, 600.0, y, moment(0))
            {
                total += lines;
            }
        }
        assert!(total < 0.0, "dragging up scrolled by {total}");

        // ...and the other way round, symmetrically.
        let mut back = Pointer::default();
        back.handle(phone(), 1, TouchPhase::Started, 600.0, 200.0, moment(0));
        let mut down = 0.0;
        for y in [300.0f32, 400.0, 500.0] {
            if let Gesture::Scrolled(lines) = back.handle(phone(), 1, TouchPhase::Moved, 600.0, y, moment(0)) {
                down += lines;
            }
        }
        assert!(down > 0.0, "dragging down scrolled by {down}");
    }

    #[test]
    fn a_drag_that_wanders_home_is_still_a_drag() {
        // The player has already scrolled the list; pressing whatever
        // has ended up under their thumb is the surprise this exists to
        // prevent.
        let out = gesture(&[
            (600.0, 600.0),
            (600.0, 300.0),
            (600.0, 600.0),
            (600.0, 600.0),
        ]);
        assert_eq!(out.last(), Some(&Gesture::Nothing), "got {out:?}");
    }

    #[test]
    fn a_cancelled_finger_presses_nothing() {
        let mut pointer = Pointer::default();
        pointer.handle(phone(), 1, TouchPhase::Started, 600.0, 400.0, moment(0));
        let out = pointer.handle(phone(), 1, TouchPhase::Cancelled, 600.0, 400.0, moment(0));
        assert_eq!(out, Gesture::Nothing);
    }

    #[test]
    fn a_second_finger_cannot_steal_a_drag_in_progress() {
        // A palm resting on the glass half way through a scroll.
        let mut pointer = Pointer::default();
        pointer.handle(phone(), 1, TouchPhase::Started, 600.0, 800.0, moment(0));
        pointer.handle(phone(), 1, TouchPhase::Moved, 600.0, 500.0, moment(0));
        pointer.handle(phone(), 2, TouchPhase::Started, 200.0, 200.0, moment(0));
        // The first finger still owns the screen, and still ends as the
        // drag it was.
        assert_eq!(
            pointer.handle(phone(), 1, TouchPhase::Ended, 600.0, 500.0, moment(0)),
            Gesture::Nothing,
        );
    }

    #[test]
    fn the_threshold_is_a_distance_rather_than_a_pixel_count() {
        // The same drag in millimetres has to mean the same thing on a
        // screen with four times the pixels.
        let small = Size { width: 1280, height: 720 };
        let large = Size { width: 2712, height: 1220 };
        for size in [small, large] {
            let short = size.height as f32;
            let mut pointer = Pointer::default();
            pointer.handle(size, 1, TouchPhase::Started, 400.0, short * 0.5, moment(0));
            // A tenth of the screen: a drag on any device.
            let moved = pointer.handle(size, 1, TouchPhase::Moved, 400.0, short * 0.4, moment(0));
            assert!(
                matches!(moved, Gesture::Scrolled(_)),
                "{size:?} did not scroll on a tenth of the screen: {moved:?}",
            );
        }
    }

    use super::*;

    /// Whether a gesture was a press of some kind, whichever kind.
    ///
    /// A test's own shorthand rather than a method on `Gesture`: the
    /// game is never allowed to ask this -- see the note where
    /// `is_click` used to be -- and a test asking it is asking about
    /// the shape of the answer, not acting on it.
    trait WasAPress {
        fn was_a_press(self) -> bool;
    }

    impl WasAPress for Gesture {
        fn was_a_press(self) -> bool {
            matches!(self, Gesture::Tapped(_))
        }
    }

    fn controls() -> Touch {
        let mut touch = Touch::default();
        touch.resize(Size::new(2400, 1080), crate::settings::TouchLayout::default(), 1.5);
        touch
    }

    #[test]
    fn a_thumb_at_rest_does_not_walk() {
        // A finger on the glass that has not moved is a hand holding a
        // phone, not a player asking to go somewhere.
        let mut touch = controls();
        touch.handle(1, TouchPhase::Started, 200.0, 800.0, moment(0));
        assert_eq!(touch.stick(), (0.0, 0.0));

        // And a twitch inside the dead zone is still not walking.
        touch.handle(1, TouchPhase::Moved, 205.0, 800.0, moment(0));
        assert_eq!(touch.stick(), (0.0, 0.0));
    }

    #[test]
    fn pushing_the_stick_forward_walks_forward() {
        let mut touch = controls();
        let (ox, oy) = (200.0, 800.0);
        touch.handle(1, TouchPhase::Started, ox, oy, moment(0));
        // Straight up the screen is forward.
        touch.handle(1, TouchPhase::Moved, ox, oy - touch.layout.stick.radius(), moment(0));
        let (x, y) = touch.stick();
        assert!(y > 0.5, "forward push gave {y}");
        assert!(x.abs() < 0.01, "forward push drifted sideways: {x}");
    }

    #[test]
    fn running_diagonally_is_not_faster_than_running_forwards() {
        // The classic bug: clamping each axis on its own lets a
        // diagonal reach 1.41x the length of a straight push, so the
        // fastest way across the world is to walk at 45 degrees.
        let mut touch = controls();
        let (ox, oy) = (200.0, 800.0);
        let far = touch.layout.stick.radius() * 4.0;

        touch.handle(1, TouchPhase::Started, ox, oy, moment(0));
        touch.handle(1, TouchPhase::Moved, ox, oy - far, moment(0));
        let straight = touch.stick();
        let straight_len = (straight.0 * straight.0 + straight.1 * straight.1).sqrt();

        touch.handle(1, TouchPhase::Ended, ox, oy - far, moment(0));
        touch.handle(2, TouchPhase::Started, ox, oy, moment(0));
        touch.handle(2, TouchPhase::Moved, ox + far, oy - far, moment(0));
        let diagonal = touch.stick();
        let diagonal_len = (diagonal.0 * diagonal.0 + diagonal.1 * diagonal.1).sqrt();

        assert!(
            (diagonal_len - straight_len).abs() < 0.02,
            "diagonal {diagonal_len} against straight {straight_len}",
        );
        assert!(straight_len <= 1.001, "the stick pushed past full: {straight_len}");
    }

    /// A moment `ms` milliseconds after an arbitrary start.
    ///
    /// The clock is handed to `handle` rather than read inside it for
    /// exactly this: place-or-mine is decided by elapsed time, and a
    /// rule about time that can only be tested by sleeping is a rule
    /// that gets tested once.
    fn moment(ms: u64) -> std::time::Instant {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        *START.get_or_init(std::time::Instant::now) + std::time::Duration::from_millis(ms)
    }

    /// Where a finger can look without landing on a button.
    ///
    /// **Searched for rather than written down.** It was a fixed point
    /// at 72% across and 25% down, which was empty glass until the
    /// buttons were spread out and the top-right corner stopped being
    /// empty. A fixture that names a coordinate is a fixture that
    /// quietly starts testing something else when the layout moves.
    fn empty_look_spot(touch: &Touch) -> (f32, f32) {
        let size = touch.layout.size.non_zero();
        let (w, h) = (size.width as f32, size.height as f32);
        for across in 1..20 {
            for down in 1..20 {
                let point = (w * across as f32 / 20.0, h * down as f32 / 20.0);
                if touch.layout.is_look_area(point.0, point.1)
                    && touch.layout.button_at(point.0, point.1).is_none()
                {
                    return point;
                }
            }
        }
        panic!("the look area is entirely covered by buttons");
    }

    /// A short tap in the look area puts a block down.
    ///
    /// The right mouse button, and the whole reason `PLACE` is not a
    /// button on the glass any more.
    #[test]
    fn a_short_tap_in_the_look_area_places_and_does_not_mine() {
        let mut touch = controls();
        let (x, y) = empty_look_spot(&touch);
        touch.handle(1, TouchPhase::Started, x, y, moment(0));
        assert!(!touch.is_mining(moment(80)), "a tap started mining");
        touch.handle(1, TouchPhase::Ended, x, y, moment(90));
        assert!(touch.take_place(), "a short tap did not place");
        assert!(!touch.take_place(), "one tap placed twice");
    }

    /// A finger that rests starts mining, and keeps mining until it
    /// lifts.
    #[test]
    fn a_resting_finger_in_the_look_area_mines_until_it_lifts() {
        let mut touch = controls();
        let (x, y) = empty_look_spot(&touch);
        touch.handle(1, TouchPhase::Started, x, y, moment(0));
        assert!(!touch.is_mining(moment(100)), "mining began before the hold was up");
        assert!(touch.is_mining(moment(300)), "a rested finger did not start mining");
        assert!(touch.is_mining(moment(5_000)), "mining stopped while the finger was down");
        touch.handle(1, TouchPhase::Ended, x, y, moment(5_100));
        assert!(!touch.is_mining(moment(5_200)), "mining outlived the finger");
        assert!(
            !touch.take_place(),
            "a finger that mined also placed a block when it lifted",
        );
    }

    /// A finger that travels turns the camera and touches nothing.
    ///
    /// The failure this stops: looking around and putting a block down
    /// at the end of every swipe, which on a phone is most of what a
    /// player does.
    #[test]
    fn a_finger_that_drags_the_camera_neither_places_nor_mines() {
        let mut touch = controls();
        let (x, y) = empty_look_spot(&touch);
        let far = touch.layout.size.non_zero().shorter() as f32 * TAP_SLOP * 4.0;
        touch.handle(1, TouchPhase::Started, x, y, moment(0));
        touch.handle(1, TouchPhase::Moved, x + far, y, moment(20));
        assert!(touch.take_look_delta() != (0.0, 0.0), "the camera did not turn");
        assert!(!touch.is_mining(moment(400)), "a drag started mining");
        touch.handle(1, TouchPhase::Ended, x + far, y, moment(500));
        assert!(!touch.take_place(), "a drag placed a block when it ended");
    }

    /// ...but looking around *while* mining keeps mining.
    ///
    /// A mouse held down does not let go because the mouse moved, and a
    /// player breaking a block is allowed to watch what they are doing.
    #[test]
    fn looking_around_while_mining_does_not_put_the_pick_down() {
        let mut touch = controls();
        let (x, y) = empty_look_spot(&touch);
        let far = touch.layout.size.non_zero().shorter() as f32 * TAP_SLOP * 4.0;
        touch.handle(1, TouchPhase::Started, x, y, moment(0));
        assert!(touch.is_mining(moment(300)));
        touch.handle(1, TouchPhase::Moved, x + far, y, moment(320));
        assert!(touch.is_mining(moment(340)), "mining stopped because the view moved");
    }

    /// A screen opening lets go of whatever the hand was doing.
    ///
    /// The same rule the buttons keep: a finger that was mining when
    /// the world stopped owning the glass must not leave the player
    /// breaking a block from inside their inventory.
    #[test]
    fn opening_a_screen_stops_the_hand_mining() {
        let mut touch = controls();
        let (x, y) = empty_look_spot(&touch);
        touch.handle(1, TouchPhase::Started, x, y, moment(0));
        assert!(touch.is_mining(moment(300)));
        touch.release_all();
        assert!(!touch.is_mining(moment(400)), "still mining after the screen took the glass");
        assert!(!touch.take_place(), "a swallowed tap was owed to the screen");
    }

    /// Which button the shipped arrangement puts shift on.
    ///
    /// Looked up by what it *sends* rather than by its index, because
    /// the index is an implementation detail of the array and the
    /// property under test is about the modifier.
    fn modifier_slot(touch: &Touch) -> Slot {
        use crate::platform::Key;
        use crate::settings::Emits;
        (0..crate::settings::TouchLayout::BUTTONS)
            .find(|slot| matches!(touch.layout.buttons[*slot].emits, Emits::Key(Key::ShiftLeft)))
            .expect("the shipped arrangement carries a modifier button")
    }

    /// "Добавь эмуляцию shift путем удерживания": the button is held,
    /// and it lets go.
    ///
    /// Both edges and nothing in between, which for *this* button is the
    /// whole of it: a modifier whose lift never arrives is a shift held
    /// down for the rest of the session, and every tap after it means
    /// something other than what the player aimed at -- a log goes onto
    /// a pile, a stack leaves the pack whole. The general rule is
    /// `a_button_reports_the_lift_as_well_as_the_press`; this is the
    /// button where breaking it is silent.
    #[test]
    fn the_modifier_button_holds_shift_down_and_lets_go_of_it() {
        let mut touch = controls();
        let slot = modifier_slot(&touch);
        assert!(touch.layout.buttons[slot].shown, "the modifier ships switched off");
        let (bx, by) = touch.layout.buttons[slot].centre;
        assert_eq!(touch.handle(1, TouchPhase::Started, bx, by, moment(0)), Hit::Pressed(slot));
        assert!(touch.is_held(slot), "the modifier was not held while the thumb was on it");
        assert_eq!(touch.handle(1, TouchPhase::Ended, bx, by, moment(900)), Hit::Released(slot));
        assert!(!touch.is_held(slot), "shift stayed down after the thumb left");
    }

    /// Holding the modifier breaks none of the three things a player was
    /// already doing.
    ///
    /// **Three fingers at once, which is the case a shared handler gets
    /// wrong.** Walking, aiming and the modifier are three zones that do
    /// not overlap, each following its own touch id; the failure this
    /// forbids is the one the module header describes -- a second or
    /// third finger landing and the first one's meaning going with it.
    #[test]
    fn the_modifier_is_held_while_the_stick_walks_and_the_other_thumb_mines() {
        let mut touch = controls();
        let slot = modifier_slot(&touch);
        let (bx, by) = touch.layout.buttons[slot].centre;

        // The left thumb is walking...
        touch.handle(1, TouchPhase::Started, 200.0, 900.0, moment(0));
        touch.handle(1, TouchPhase::Moved, 200.0, 900.0 - touch.layout.stick.radius(), moment(5));
        // ...a second finger takes the modifier...
        assert_eq!(touch.handle(2, TouchPhase::Started, bx, by, moment(10)), Hit::Pressed(slot));
        // ...and the right thumb rests on the world and breaks a block.
        let (lx, ly) = empty_look_spot(&touch);
        touch.handle(3, TouchPhase::Started, lx, ly, moment(20));

        assert!(touch.is_mining(moment(400)), "the pick stopped swinging while shift was held");
        assert!(touch.stick().1 > 0.5, "the walk stopped when the modifier went down");
        assert!(touch.is_held(slot), "the modifier let go when another finger landed");

        // And letting the modifier go leaves the other two alone.
        assert_eq!(touch.handle(2, TouchPhase::Ended, bx, by, moment(500)), Hit::Released(slot));
        assert!(touch.is_mining(moment(520)), "letting shift go put the pick down");
        assert!(touch.stick().1 > 0.5, "letting shift go stopped the walk");
    }

    #[test]
    fn a_finger_on_a_button_does_not_also_turn_the_camera() {
        // The buttons sit inside the look area. Tested first, or every
        // tap on dig would also spin the view.
        let mut touch = controls();
        let (bx, by) = touch.layout.buttons[0].centre;
        assert_eq!(touch.handle(1, TouchPhase::Started, bx, by, moment(0)), Hit::Pressed(0));
        touch.handle(1, TouchPhase::Moved, bx + 2.0, by + 2.0, moment(0));
        assert_eq!(touch.take_look_delta(), (0.0, 0.0));
        assert!(touch.is_held(0));
    }

    #[test]
    fn a_thumb_that_slides_off_a_button_stops_pressing_it() {
        let mut touch = controls();
        let (bx, by) = touch.layout.buttons[2].centre;
        touch.handle(1, TouchPhase::Started, bx, by, moment(0));
        assert!(touch.is_held(2));
        touch.handle(1, TouchPhase::Moved, bx, by - touch.layout.buttons[0].radius() * 8.0, moment(0));
        assert!(!touch.is_held(2), "the button stayed held after the thumb left");
    }

    #[test]
    fn dragging_the_right_of_the_screen_turns_the_camera() {
        let mut touch = controls();
        touch.handle(1, TouchPhase::Started, 1600.0, 400.0, moment(0));
        touch.handle(1, TouchPhase::Moved, 1650.0, 420.0, moment(0));
        let (dx, dy) = touch.take_look_delta();
        assert_eq!((dx, dy), (50.0, 20.0));
        // Drained, not remembered: the camera has had it.
        assert_eq!(touch.take_look_delta(), (0.0, 0.0));
    }

    #[test]
    fn losing_the_window_forgets_every_finger() {
        // Touch-up never arrives for a finger that was down when the
        // activity went away, so without this the player comes back
        // walking into a wall.
        let mut touch = controls();
        touch.handle(1, TouchPhase::Started, 200.0, 800.0, moment(0));
        touch.handle(1, TouchPhase::Moved, 200.0, 600.0, moment(0));
        touch.handle(2, TouchPhase::Started, touch.layout.buttons[0].centre.0, touch.layout.buttons[0].centre.1, moment(0));
        touch.release_all();
        assert_eq!(touch.stick(), (0.0, 0.0));
        assert!(!touch.is_held(0));
        assert_eq!(touch.take_look_delta(), (0.0, 0.0));
    }

    // ---- the two-finger and the rested press ----

    /// The finger already on the glass is the modifier; the one that
    /// lands after it is the click.
    ///
    /// This is the whole of shift-clicking on a phone. Without it the
    /// only way to move forty pieces of flint out of a chest was forty
    /// taps -- and "a mechanic should create a decision, not a chore"
    /// is the rule this game settles arguments with.
    #[test]
    fn a_finger_already_on_the_glass_makes_the_next_tap_move_the_whole_stack() {
        let mut pointer = Pointer::default();
        // The modifier lands first, the way a hand reaches for shift.
        pointer.handle(phone(), 1, TouchPhase::Started, 300.0, 900.0, moment(0));
        pointer.handle(phone(), 2, TouchPhase::Started, 1400.0, 500.0, moment(120));
        assert_eq!(
            pointer.handle(phone(), 2, TouchPhase::Ended, 1402.0, 501.0, moment(200)),
            Gesture::Tapped(Chord::Quick),
        );
        // ...and the hand that was holding it is not a second click.
        assert_eq!(
            pointer.handle(phone(), 1, TouchPhase::Ended, 300.0, 900.0, moment(260)),
            Gesture::Nothing,
            "the modifier finger clicked as well, so one gesture did two things",
        );
    }

    /// A modifier that is still down modifies the next tap too.
    ///
    /// The point of holding it rather than tapping it: unloading a
    /// chest is a run of these, and a modifier that had to be renewed
    /// between every one would be the chore back again in a new shape.
    #[test]
    fn the_modifier_finger_keeps_modifying_until_it_leaves() {
        let mut pointer = Pointer::default();
        pointer.handle(phone(), 1, TouchPhase::Started, 300.0, 900.0, moment(0));
        for (finger, at) in [(2, 120u64), (3, 400), (4, 700)] {
            pointer.handle(phone(), finger, TouchPhase::Started, 1400.0, 500.0, moment(at));
            assert_eq!(
                pointer.handle(phone(), finger, TouchPhase::Ended, 1400.0, 500.0, moment(at + 80)),
                Gesture::Tapped(Chord::Quick),
                "tap {finger} was not modified",
            );
        }
    }

    /// Lifting the modifier first is a change of mind, not a click.
    ///
    /// **Where the click would have landed is the whole argument.** The
    /// modifier finger is resting wherever it fell -- for a right-handed
    /// player, off the panel entirely, which is where "cancel" and
    /// "close" live on every screen in this game. A rule that let the
    /// elder finger click would put a cancel under a thumb that was
    /// only being a modifier.
    ///
    /// The younger one is still on the glass and still on whatever it
    /// was pointing at, so it stays live and its own lift is an
    /// ordinary press.
    #[test]
    fn lifting_the_modifier_first_presses_nothing_and_leaves_the_other_finger_working() {
        let mut pointer = Pointer::default();
        pointer.handle(phone(), 1, TouchPhase::Started, 300.0, 900.0, moment(0));
        pointer.handle(phone(), 2, TouchPhase::Started, 1400.0, 500.0, moment(120));
        assert_eq!(
            pointer.handle(phone(), 1, TouchPhase::Ended, 300.0, 900.0, moment(200)),
            Gesture::Nothing,
        );
        assert_eq!(
            pointer.handle(phone(), 2, TouchPhase::Ended, 1400.0, 500.0, moment(300)),
            Gesture::Tapped(Chord::Plain),
            "the finger that was left could no longer press anything",
        );
    }

    /// A finger that rests and then lifts is the other mouse button.
    ///
    /// Half of one stack rather than all of it: the right button, which
    /// a phone has no way of naming otherwise. Half a second, because
    /// that is what Android has called a long press since there were
    /// Androids, and an ordinary tap has to stay an ordinary tap.
    #[test]
    fn a_finger_that_rests_before_it_lifts_is_the_other_button() {
        for (held_for, expected) in [
            (90u64, Chord::Plain),
            (200, Chord::Plain),
            (499, Chord::Plain),
            (500, Chord::Secondary),
            (900, Chord::Secondary),
        ] {
            let mut pointer = Pointer::default();
            pointer.handle(phone(), 1, TouchPhase::Started, 1400.0, 500.0, moment(0));
            assert_eq!(
                pointer.handle(phone(), 1, TouchPhase::Ended, 1403.0, 498.0, moment(held_for)),
                Gesture::Tapped(expected),
                "a press held for {held_for} ms",
            );
        }
    }

    /// A palm landing during a scroll can neither modify nor press.
    ///
    /// The rule that was already here -- a second finger cannot steal a
    /// drag in progress -- said nothing about a second finger that
    /// *is* the drag's undoing. Now that a second finger means
    /// something, the palm has to be refused twice: it must not become
    /// a modifier, and its own lift must not become the press that the
    /// scroll was never going to be.
    #[test]
    fn a_palm_landing_during_a_scroll_neither_modifies_nor_presses() {
        let mut pointer = Pointer::default();
        pointer.handle(phone(), 1, TouchPhase::Started, 600.0, 900.0, moment(0));
        pointer.handle(phone(), 1, TouchPhase::Moved, 600.0, 400.0, moment(100));
        pointer.handle(phone(), 2, TouchPhase::Started, 200.0, 200.0, moment(150));
        assert_eq!(
            pointer.handle(phone(), 2, TouchPhase::Ended, 200.0, 200.0, moment(200)),
            Gesture::Nothing,
            "the palm pressed something at the end of a scroll",
        );
        assert_eq!(
            pointer.handle(phone(), 1, TouchPhase::Ended, 600.0, 400.0, moment(260)),
            Gesture::Nothing,
        );
    }

    /// A modifier finger does not scroll the list as well.
    ///
    /// Two fingers each handing out lines would move a list twice as far
    /// as either of them travelled, which is a list that outruns the
    /// hand on it.
    #[test]
    fn only_one_of_two_fingers_scrolls() {
        let mut pointer = Pointer::default();
        pointer.handle(phone(), 1, TouchPhase::Started, 600.0, 900.0, moment(0));
        pointer.handle(phone(), 2, TouchPhase::Started, 900.0, 900.0, moment(50));
        let mut lines = 0.0;
        for (finger, y) in [(1, 800.0f32), (2, 800.0), (1, 700.0), (2, 700.0)] {
            if let Gesture::Scrolled(by) =
                pointer.handle(phone(), finger, TouchPhase::Moved, 600.0, y, moment(100))
            {
                lines += by;
            }
        }
        // The first finger travelled two hundred pixels, which at this
        // size is three lines. Had the second been listened to as well
        // it would be six.
        let short = phone().shorter() as f32;
        let expected = (-200.0f32 / (SCROLL_LINE * short)).trunc();
        assert_eq!(lines, expected, "both fingers scrolled the same list");
    }

    // ---- the wheel ----

    /// Which slot opens the wheel in the arrangement the game ships.
    fn hub_slot() -> Slot {
        let arrangement = crate::settings::TouchLayout::default();
        (0..crate::settings::TouchLayout::BUTTONS)
            .find(|slot| matches!(arrangement.buttons[*slot].emits, crate::settings::Emits::More))
            .expect("the shipped arrangement has a wheel")
    }

    /// ...and which slots are inside it.
    fn member_slots() -> Vec<Slot> {
        let arrangement = crate::settings::TouchLayout::default();
        (0..crate::settings::TouchLayout::BUTTONS)
            .filter(|slot| arrangement.buttons[*slot].in_wheel)
            .collect()
    }

    /// A shut wheel is not a set of invisible buttons.
    ///
    /// The failure this exists for is the one the arrangement is laid
    /// out to avoid in the first place: a thumb swinging for the look
    /// area opening the chat box. A member that were hit-tested while
    /// it is not drawn would be exactly that, and worse, because there
    /// would be nothing on the glass to explain it.
    #[test]
    fn a_shut_wheel_has_nothing_on_the_glass_to_press() {
        let shut = Layout::for_size(phone(), crate::settings::TouchLayout::default(), 1.5, false);
        for slot in member_slots() {
            assert!(!shut.buttons[slot].shown, "member {slot} is drawn with the wheel shut");
            let (x, y) = shut.buttons[slot].centre;
            assert_ne!(
                shut.button_at(x, y),
                Some(slot),
                "member {slot} answered a touch through a shut wheel",
            );
        }
    }

    /// ...and an open one is pressable exactly where it is drawn.
    ///
    /// The inverse of the drawing, which is the rule this whole
    /// interface is held to: `hud::touch_controls` draws every button
    /// the layout says is shown, at the box the layout gives it, so
    /// this is the same rectangle from the other side.
    #[test]
    fn every_button_the_open_wheel_puts_out_is_pressed_where_it_is_drawn() {
        let open = Layout::for_size(phone(), crate::settings::TouchLayout::default(), 1.5, true);
        let members = member_slots();
        assert!(members.len() > 1, "a wheel of one is not a wheel");
        for slot in &members {
            assert!(open.buttons[*slot].shown);
            let (x, y) = open.buttons[*slot].centre;
            assert_eq!(open.button_at(x, y), Some(*slot));
        }
        // ...and the members do not sit on top of one another or on the
        // hub, which is what an arc that is too small looks like.
        let mut all = members.clone();
        all.push(hub_slot());
        for (index, a) in all.iter().enumerate() {
            for b in &all[index + 1..] {
                let (one, two) = (&open.buttons[*a], &open.buttons[*b]);
                let apart_x = (one.centre.0 - two.centre.0).abs() - (one.half.0 + two.half.0);
                let apart_y = (one.centre.1 - two.centre.1).abs() - (one.half.1 + two.half.1);
                assert!(
                    apart_x > 0.0 || apart_y > 0.0,
                    "wheel buttons {a} and {b} overlap: {one:?} against {two:?}",
                );
            }
        }
    }

    /// Pressing the `...` button opens the wheel and pressing it again
    /// puts it away.
    ///
    /// On the press rather than on the lift, because everything else on
    /// this glass happens when the thumb lands, and one control that
    /// waits for the lift is one control that feels broken.
    #[test]
    fn the_wheel_opens_under_the_thumb_that_asked_for_it() {
        let mut touch = controls();
        let hub = hub_slot();
        let (x, y) = touch.layout.buttons[hub].centre;

        assert_eq!(touch.handle(1, TouchPhase::Started, x, y, moment(0)), Hit::Pressed(hub));
        assert!(touch.wheel_open, "the wheel did not open");
        for slot in member_slots() {
            assert!(touch.layout.buttons[slot].shown, "member {slot} stayed away");
        }
        touch.handle(1, TouchPhase::Ended, x, y, moment(80));
        assert!(touch.wheel_open, "the wheel shut again when the thumb left it");

        touch.handle(2, TouchPhase::Started, x, y, moment(400));
        assert!(!touch.wheel_open, "a second press did not put the wheel away");
    }

    /// Choosing something from the wheel puts the wheel away.
    ///
    /// On the lift and not on the press, so the button is still there,
    /// and still drawn as held, for as long as the thumb is on it --
    /// a control that vanishes underneath the finger pressing it gives
    /// no sign that it was pressed at all.
    #[test]
    fn choosing_from_the_wheel_shuts_it_when_the_thumb_lifts() {
        let mut touch = controls();
        let hub = hub_slot();
        let member = member_slots()[0];
        let (hx, hy) = touch.layout.buttons[hub].centre;
        touch.handle(1, TouchPhase::Started, hx, hy, moment(0));
        touch.handle(1, TouchPhase::Ended, hx, hy, moment(80));

        let (mx, my) = touch.layout.buttons[member].centre;
        assert_eq!(
            touch.handle(2, TouchPhase::Started, mx, my, moment(200)),
            Hit::Pressed(member),
        );
        assert!(touch.wheel_open, "the wheel went away under the thumb pressing it");
        assert_eq!(
            touch.handle(2, TouchPhase::Ended, mx, my, moment(280)),
            Hit::Released(member),
            "the key the member emulates was never released",
        );
        assert!(!touch.wheel_open, "the wheel stayed open after a choice was made");
    }

    /// A touch beside an open wheel puts it away and does nothing else.
    ///
    /// Dismissing is what a finger outside an open menu means on every
    /// phone there is. Doing nothing else with the same touch is the
    /// part worth stating: the wheel sits over the look area, so a
    /// dismissal that also counted as a tap there would put a block in
    /// a wall the player never meant to build.
    #[test]
    fn a_touch_beside_an_open_wheel_dismisses_it_and_places_nothing() {
        let mut touch = controls();
        let hub = hub_slot();
        let (hx, hy) = touch.layout.buttons[hub].centre;
        touch.handle(1, TouchPhase::Started, hx, hy, moment(0));
        touch.handle(1, TouchPhase::Ended, hx, hy, moment(80));
        assert!(touch.wheel_open);

        let (x, y) = empty_look_spot(&touch);
        touch.handle(2, TouchPhase::Started, x, y, moment(200));
        assert!(!touch.wheel_open, "the wheel ignored a touch beside it");
        assert!(!touch.is_mining(moment(900)), "the dismissal started mining");
        touch.handle(2, TouchPhase::Ended, x, y, moment(950));
        assert!(!touch.take_place(), "the dismissal put a block down");
    }

    /// A screen opening takes the wheel with it.
    ///
    /// A player who opens their pack and comes back to a wheel they
    /// left out two minutes ago has been handed a control they did not
    /// choose, sitting where they are about to aim.
    #[test]
    fn the_wheel_does_not_survive_the_world_losing_the_glass() {
        let mut touch = controls();
        let hub = hub_slot();
        let (hx, hy) = touch.layout.buttons[hub].centre;
        touch.handle(1, TouchPhase::Started, hx, hy, moment(0));
        assert!(touch.wheel_open);
        touch.release_all();
        assert!(!touch.wheel_open);
        for slot in member_slots() {
            assert!(!touch.layout.buttons[slot].shown);
        }
    }

    /// A thumb that slides off a button lets go of the key it was
    /// holding.
    ///
    /// **The release used to be dropped on the floor here**, and the
    /// bug was the one `Hit::Released` exists to prevent, one level
    /// further in: the slot came out of `held` and nothing was
    /// returned, so the key the button emulates was never released. A
    /// thumb that pressed JUMP and slid off towards the stick left
    /// SPACE down for the rest of the session.
    #[test]
    fn a_thumb_sliding_off_a_button_reports_letting_go_of_it() {
        let mut touch = controls();
        let (bx, by) = touch.layout.buttons[0].centre;
        touch.handle(1, TouchPhase::Started, bx, by, moment(0));
        // Somewhere that is on no button at all, found rather than
        // guessed: the first version of this slid straight up and
        // landed on the wheel, which is a finger that moved from one
        // button to another and is not what this is about.
        let (ax, ay) = empty_look_spot(&touch);
        assert_eq!(
            touch.handle(1, TouchPhase::Moved, ax, ay, moment(60)),
            Hit::Released(0),
            "the button stopped being held and nobody was told",
        );
        assert!(!touch.is_held(0));
    }

    #[test]
    fn the_controls_stay_on_screen_whatever_the_shape() {
        // A phone is not one aspect ratio. Every control has to be
        // somewhere a thumb can reach and the glass actually is.
        for ((w, h), wheel_open) in [(2400, 1080), (1080, 2400), (1280, 720), (2960, 1440)]
            .into_iter()
            .flat_map(|shape| [(shape, false), (shape, true)])
        {
            let layout = Layout::for_size(
                Size::new(w, h),
                crate::settings::TouchLayout::default(),
                1.5,
                wheel_open,
            );
            let (fw, fh) = (w as f32, h as f32);
            let on_screen = |(x, y): (f32, f32), r: f32| {
                x - r >= 0.0 && y - r >= 0.0 && x + r <= fw && y + r <= fh
            };
            assert!(
                on_screen(layout.stick.centre, layout.stick.radius()),
                "stick off screen at {w}x{h}",
            );
            for (index, button) in layout.buttons.iter().enumerate() {
                assert!(
                    on_screen(button.centre, button.radius()),
                    "button {index} off screen at {w}x{h}",
                );
            }
        }
    }
}

