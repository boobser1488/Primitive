//! What a falling block does to whoever is under it.
//!
//! The falling simulation knows blocks and knows nothing about bodies;
//! the tick loop knows where everybody is. This is the seam between
//! them: once a tick, after the blocks have moved, everybody's box is
//! handed to `FallingBlocks::strikes` and every blow it reports is
//! applied through the same path a boar's tusk takes -- `strike_player`,
//! so armour counts and a helmet is worth wearing in a mine, and
//! `report_vitals`, so the death screen says what happened and the pack
//! ends up on the floor of the gallery rather than nowhere.
//!
//! It lives in its own module rather than in the tick loop's body for
//! the reason the animals' blows do: the tick loop is the one function
//! everything has to fit into, and every mechanic that grows a paragraph
//! there makes the next one harder to read.
//!
//! ## Locks
//!
//! Three are touched and never two at once: the player states and the
//! animals to learn where everyone is, the simulation to ask what hit
//! them, the player states again to hurt them. `strike_player` fires a
//! mod hook, and a hook called under a lock is the one rule of that
//! subsystem broken (see `logic::mods`), so the blows are collected as
//! data with every guard closed and applied afterwards.

use std::sync::Arc;

use primitive_shared::geometry::{PLAYER_HALF_WIDTH, PLAYER_HEIGHT};
use primitive_shared::protocol::EntityKind;

use crate::logic::falling::{crush_cause, Body, BodyKind};
use crate::logic::survival;
use crate::Context;

/// Hurts everyone a falling block passed through or landed on since the
/// last tick. Call once per tick, after the simulation has stepped.
pub(crate) fn crush_whoever_is_under(ctx: &Arc<Context>) {
    // Nothing in the air is the case on nearly every tick, and it costs
    // one lock and two lengths rather than a lock per player.
    {
        let sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
        if !sim.may_strike() {
            return;
        }
    }

    let mut bodies: Vec<Body> = Vec::new();
    for handle in ctx.registry.handles() {
        let state = handle.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.vitals.is_dead() {
            continue;
        }
        let (x, y, z) = state.position;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            continue;
        }
        bodies.push(Body {
            kind: BodyKind::Player,
            id: handle.id,
            feet: primitive_shared::geometry::narrow(state.position),
            half_width: PLAYER_HALF_WIDTH,
            height: PLAYER_HEIGHT,
        });
    }
    {
        let animals = ctx.animals.lock().unwrap_or_else(|e| e.into_inner());
        for state in animals.states() {
            let EntityKind::Animal { species, .. } = state.kind else {
                continue;
            };
            let (hx, _, hz) = species.half_extents();
            bodies.push(Body {
                kind: BodyKind::Animal,
                id: state.id,
                feet: primitive_shared::geometry::narrow((state.x, state.y, state.z)),
                half_width: hx.max(hz),
                height: species.height(),
            });
        }
    }

    let blows = {
        let mut sim = ctx.falling.lock().unwrap_or_else(|e| e.into_inner());
        sim.strikes(&bodies)
    };

    for blow in blows {
        match blow.kind {
            BodyKind::Player => {
                let Some(victim) = ctx.registry.get(blow.id) else {
                    continue;
                };
                // Something heavy arriving from above: it bruises, and a
                // heavy enough one breaks the arm it lands on. See
                // `injury::Blow::Crush`.
                let outcome = crate::strike_player(
                    ctx,
                    &victim,
                    blow.damage,
                    crush_cause(blow.block),
                    primitive_shared::injury::Blow::Crush,
                );
                if !matches!(outcome, survival::Outcome::Unchanged) {
                    crate::report_vitals(ctx, &victim, outcome);
                }
            }
            BodyKind::Animal => {
                // No armour on a deer, so straight to `hurt`. A death
                // is announced by the animals' own step, which drains
                // `take_deaths` -- and what it leaves on the ground is laid
                // there too, once the body has fallen (`animals::FALL_SECONDS`),
                // as it is for a spear.
                let _ = ctx.animals.lock().unwrap_or_else(|e| e.into_inner()).hurt(blow.id, blow.damage);
            }
        }
    }
}
