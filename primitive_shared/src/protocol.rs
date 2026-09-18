//! Wire protocol.
//!
//! Shape of this version (v2), and why:
//!
//! * **Versioned handshake.** The very first thing a client sends is
//!   `Hello`; the server answers `Welcome` or `Rejected`. Anything else
//!   before the handshake is a protocol error. This is what lets an old
//!   client be told "you're out of date" instead of silently desyncing
//!   once the message layout changes.
//! * **Batching.** `RequestChunks`, `BlockUpdates` and `Snapshot` carry
//!   many items per message. At 1 player it makes no difference; at 500
//!   it's the difference between 20 relayed messages per player per tick
//!   and 1.
//! * **Tick-based snapshots.** Player movement is no longer relayed
//!   message-for-message. The server samples every player once per tick
//!   and sends each of them one `Snapshot` containing only the players
//!   inside their interest radius -- O(nearby), not O(all players).
//! * **Server-authoritative correction.** `PositionCorrection` exists so
//!   the anti-cheat can reject a move instead of only logging it.

use serde::{Deserialize, Serialize};

use crate::types::{BlockId, Chunk, ChunkPos};

pub type PlayerId = u64;

/// Bumped on any incompatible change to the messages below. The server
/// refuses a client whose version doesn't match.
///
/// v3 added survival: `Health`, `Died`, `Respawned` and `Respawn`.
/// v4 added `CarriedWeight`, which fed fall damage.
/// v5 moved the inventory to the server: `InventoryState` replaces it,
/// along with `SwapSlots`, `DropSlot`, `Craft` and `SelectSlot`, and
/// `EntityKind::Item` carries dropped stacks in the world.
/// v6 gave the inventory screen the rest of its gestures: `SwapSlots`
/// became `MoveSlots` (which merges rather than only swapping), and
/// `SplitSlot`, `QuickMoveSlot` and `SortInventory` joined it. `Craft`
/// gained `times`.
/// v7 changed nothing about the messages, and is a bump anyway: fibre
/// is a new block id, pulling up grass yields it instead of the tuft,
/// and the recipe table gained two entries while `thatch` changed what
/// it costs. `Craft` names a recipe by its index into that table, so an
/// older client asking for recipe 7 would be spending the wrong
/// ingredients -- which the message format cannot express, and only a
/// version check can catch.
/// v8 added loose stones: a new block id, a tenth recipe, and a third
/// block shape. Same reason as v7 -- `Craft` names a recipe by its index
/// into a table both sides have to agree on, and an older client would
/// be spending the wrong ingredients.
/// v9 changed the rules rather than the messages: rock and standing
/// timber cannot be broken by hand, dressed stone is no longer
/// placeable, and recipe 2 now splits cobble into stones instead of
/// pressing stones into a block. A recipe is named by its index, so two
/// sides disagreeing about what index 2 means is two sides spending
/// different ingredients.
/// v10 added `Attack`, and with it the first damage in the game that is
/// not the ground. Flint is a new block id and a twelfth recipe, and
/// the terrain generator was rebuilt -- the seed is on the wire, so two
/// sides disagreeing about what a seed means is a client drawing a
/// world the server does not have.
/// v11 added chests: `OpenChest`, `CloseChest`, `ChestMove`,
/// `ChestQuickMove` and `ChestState`, and a block id to go with them.
/// The chest is also the first block whose *contents* are server state,
/// which is why the gestures name a side rather than a slot index --
/// see `Side`.
/// v12 also added `ChestBulkMove`: storing forty slots one message at a
/// time is what the rate limit is for, and a transfer that half
/// happened because a message was dropped is worse than one that did
/// not. It changed what a block id *means* rather than adding a message:
/// loose material carries a depth in the same field a log carries its
/// axis in, so a cell of sand may be any of eight heights. Nothing on
/// the wire is a different shape, and that is exactly why this had to
/// be a version bump -- an older client would take a three-eighths
/// drift of snow for a whole block, draw it as one, walk on top of it,
/// and aim at a metre of empty air above it. A refused handshake says
/// so; a world that is quietly the wrong shape does not.
/// v14 added the backpack: a new block id, a second container, and a new
/// thing that happens when you die -- the pack goes into a block at the
/// death site instead of staying on the corpse. No message changed
/// shape, and that is once again exactly why the version had to move. An
/// older client would meet an id it has no row for, draw a dead player's
/// belongings as the unknown-block placeholder, and refuse to open the
/// one thing in the world worth opening; a newer client against an older
/// server would keep expecting to respawn with its pack. Neither is
/// something the message layout can express, and a refused handshake is
/// the only place either can be caught.
/// v15 added ore, metal and tools: thirteen block ids, eight recipes at
/// the end of the table, and a rule about mining that did not exist
/// before -- what you are *holding* now decides whether a block gives
/// way at all.
///
/// No message changed shape, and as with v12 and v14 that is precisely
/// why the number had to move. The disagreements an older peer would
/// have are all invisible ones. An older client would meet copper ore
/// and draw the unknown-block placeholder into the middle of a hillside;
/// worse, it computes its own mining progress, so against this server it
/// would fill a progress bar on a rock it has no tool for and have every
/// swing refused, which reads as the game being broken rather than as a
/// rule. A newer client against an older server would ask for recipes by
/// indices that server does not have. A refused handshake is the only
/// place any of that can be caught.
/// v16 took the metal picks out and put a stone age in: three flint
/// tools instead of a four-rung pick ladder, seven new block ids for the
/// parts they are assembled from, and eight recipes **in the middle of
/// the table** where the one-click flint pick used to be.
///
/// That last part is why this bump is not optional and not cosmetic.
/// `Craft` names a recipe by its index, and every index from the flint
/// pick onwards now means something else -- an older client asking for
/// what it thinks is "copper ingot" would be spending a player's flint
/// on a knife head. Nor could an older client survive the block ids: it
/// would find a knife in a pack it has no row for and draw the unknown
/// placeholder, and it computes its own mining progress, so it would
/// fill a bar on a standing tree it believes nothing can fell and have
/// every swing refused. A newer client against an older server is the
/// mirror image, asking for recipes past the end of that server's
/// table. None of it changes the shape of a message; all of it is
/// caught by the handshake and nowhere else.
/// v17 is the first bump in a while that changes the shape of messages
/// as well as the meaning of the old ones, because 1.5 added things the
/// protocol had no way to say at all.
///
/// New from the client: `Eat`, which spends a slot on a stomach;
/// `UseBlock`, which is the right click that is neither a placement nor
/// a chest -- lighting a fire, feeding it; and `AttackEntity`, which is
/// `Attack` aimed at something that is not a player. New from the
/// server: `Nourishment`, the second bar on the HUD; `WeatherSync`,
/// which is the world clock's counterpart for the sky; and a third
/// `EntityKind`, `Animal`, which carries a species and a facing.
///
/// The invisible half is as usual the more dangerous one. Twenty new
/// block ids -- four plants, four foods, two fires, nine metal tools --
/// none of which an older client has a row for, and it would draw the
/// unknown-block placeholder into the middle of a meadow. Twenty-odd
/// recipes appended, which an older client cannot ask for and a newer
/// one would ask a v16 server for by an index past the end of its table.
/// A rule about where a recipe may be run at all, which the older client
/// does not know and would offer anywhere. And iron ore, which now asks
/// for a copper pick: a v16 client would fill a progress bar on it with
/// flint in hand and have every swing refused, which reads as the game
/// being broken rather than as an age it has not reached.
///
/// # 18: the kiln and the wolf
///
/// Nothing new in the message list at all, and the version still had to
/// go up -- which is exactly the case this constant exists for, and the
/// one it is easiest to skip.
///
/// Four things crossed the wire differently. **`Species` grew a fourth
/// variant**, and an enum on the wire is an index: a v17 client handed a
/// `Wolf` decodes a value its table does not have and drops the frame
/// the whole entity snapshot came in, so every animal in sight vanishes
/// rather than one being drawn wrong. **Four block ids** a v17 client
/// has no row for -- two kilns, a brick and brickwork -- which it would
/// draw as the unknown-block placeholder in the middle of somebody's
/// house. **Four recipes appended**, which a v17 server would look up by
/// an index past the end of its table. And **thirteen recipes moved from
/// the campfire to the kiln**, which is the dangerous one, because it is
/// invisible: a v17 client would offer smelting at a campfire, send the
/// craft, and have the server refuse it with no way to explain why.
///
/// # 19: the field and the crucible
///
/// Sixteen more block ids, eleven more recipes, two more stations and a
/// third refusal in the crafting menu -- and, the dangerous one again,
/// **thirteen recipes changed where they may be run**. A v18 client
/// would offer smelting at a kiln with no crucible in the pack, send the
/// craft, and be refused with nothing on screen to explain it.
///
/// # 20: ice, and a world that is not made of noise
///
/// One more block id -- and, the part that makes this a version rather
/// than a block, **a `Welcome` that carries which generator the world
/// came out of**. The client builds its own copy of the generator from
/// the seed, and used to have no way to be told that the seed was not
/// the whole answer: against a v20 server running the test preset a v19
/// client would tint the field from noise the server never read and name
/// a biome that is not there. The field is in the handshake because that
/// is where every other fact about the world already is.
///
/// # 21: the fire has an inside
///
/// Three hearths became containers, which changes what a right click on
/// one does and adds a screen with slots in it -- so `ChestState` now
/// carries *which kind* of container it is and, for a hearth, how much
/// of its fuel is left and how far along the batch is. A v20 client
/// handed one would draw a forty-slot chest with four things scattered
/// in it and no way to see the fire.
///
/// It also **takes the fire recipes out of the player's crafting list**,
/// which is the dangerous half again: a v20 client would offer smelting
/// to a player standing beside a lit kiln, send the craft, and be
/// refused by a server that no longer runs those recipes for hands.
///
/// # 22: tools wear out
///
/// A slot on the wire went from two numbers to three -- what it holds,
/// how many, and how worn (see `inventory::Stack::damage`) -- and
/// bincode writes fields by position, so a v21 client handed a v22
/// inventory does not fail, it reads the wrong bytes into the wrong
/// fields and draws a pack full of nonsense.
///
/// # 25: the server can grant flight
///
/// `Flight` is a new message, and a new *capability*: a client that
/// receives it stops applying gravity to itself until it is told
/// otherwise. A v24 client handed one fails to decode rather than
/// misreading it -- the variant is past the end of the enum it knows --
/// which is the safe half of the failure. The dangerous half is the
/// other direction: a v24 *server* would never send it, so a v25 client
/// would sit there with the setting it was never granted, which is
/// nothing at all. The bump is for the first case.
///
/// # 24: a rack is a container
///
/// The drying rack grew a screen, and with it two slots in the ordinary
/// container store (see `crate::rack`). A v23 client at a rack asks to
/// *use* the block and gets a container screen it has no layout for;
/// worse, `ChestState` grew a field, and bincode writes fields by
/// position -- so a v23 client handed a v24 container message does not
/// fail, it reads the wrong bytes into the wrong fields.
/// # 26: the client can ask what is extending the server
///
/// `RequestExtensions` and `Extensions` are two new messages, and
/// nothing else moved. What they carry is what `/mods` has always
/// printed -- see `ExtensionList` for why it is a structure rather than
/// the lines that command produces.
///
/// The bump is for the direction that is not a decode failure. A v25
/// client asking a v26 server nothing is fine; a v26 client asking a
/// v25 server would wait for an answer that is never coming, with a
/// screen open and no way to tell "this server has no mods" from "this
/// server does not know the question". A refused handshake says which.
// 27: the calendar (`world_days` on `Welcome` and `TimeSync`), and the
// recipe table grew in the middle -- cord, glue, the spear -- which
// renumbers every recipe index after them on the wire.
//
// # 28: a jug carries things
//
// `PourIntoJug` and `EmptyJug` are two new `ClientMessage` variants,
// and they are in the middle of the enum rather than after it --
// bincode writes a variant as its *index*, so every message past
// `Unequip` is renumbered and a v27 server handed a v28 `Respawn` would
// read it as something else entirely. That alone is the bump.
//
// The quieter half is that `inventory::Stack::damage` grew a second
// meaning (see `inventory::jug_contents`): the same three numbers on
// the wire, but a v27 client shown a v28 jug would draw a wear bar's
// worth of nonsense and, worse, would offer to drag the jug onto
// another one -- which a v27 server would happily merge, because it
// still thinks a jug stacks to four. A refused handshake is the only
// honest answer to that.
//
// # 29: other people are dressed
//
// `PlayerState` grew an [`Outfit`] -- four worn garments and whatever is
// in the hand -- and bincode writes fields by position, so a v28 client
// handed a v29 snapshot does not fail: it reads the next player's id out
// of this player's sleeves. That alone is the bump.
//
// The quieter half is the direction that decodes cleanly. A v29 client
// against a v28 server would find every player bare and empty-handed,
// which is not an error anywhere -- it is exactly what the wire says
// about somebody who has nothing on. A world where nobody can be seen
// wearing what they are wearing is not something a client can detect,
// and a refused handshake is the only place it can be caught.
// # 30: a body gets tired, and can lie down
//
// [`ServerMessage::Body`] grew a fourth number -- how tired the player
// is -- and bincode writes fields by position, so a v29 client reading a
// v30 body would take the fatigue for whatever came next in the stream.
// That alone is the bump.
//
// [`ServerMessage::Asleep`] is the other half and is the reason it could
// not be a client-side effect: while a player is asleep the server
// ignores what their client says about where they are, so a client that
// did not know it was asleep would predict a walk, be corrected, and
// fight the server twenty times a second. The message is what stops the
// prediction.
// # 31: a leg can be broken
//
// [`ServerMessage::Body`] grew a fifth field, and bincode writes fields
// by position: a v30 client reading a v31 body takes the flag for
// whatever comes next in the stream. The flag has to be on the wire at
// all because a limp is *predicted* -- the client applies the speed
// itself, or every step of a broken-legged player is a correction from
// the server twenty times a second.
/// v32: the world is 256 blocks tall and a chunk's blocks travel
/// run-length encoded (`types::rle_blocks`). An old client would read
/// the runs as a raw array and draw noise, so the version says no first.
///
/// v33: the savanna's animals. `Species` grew three variants (zebra,
/// antelope, lion) and bincode sends a species as its index, so a v32
/// client meeting a lion reads an index it has no variant for and drops
/// the whole entity snapshot -- every animal near that player vanishes,
/// not only the lion (the v18 note). Four block ids came with them (the
/// three carcasses and `BLOCK_BONES_2`), which an old client would draw
/// as the unknown-block placeholder. Neither is a crash anywhere, and a
/// refused handshake is the only place it can be caught.
///
/// v34: the experimental trees. Two block ids (`BLOCK_TWIG`,
/// `BLOCK_BOUGH`) that an old client would draw as the unknown-block
/// placeholder -- a forest of grey cubes -- and a third `Preset`, which
/// the handshake carries by index: a v33 client told the world is
/// `branches` fails to read the welcome at all, which is a worse way to
/// learn it than a version refusal.
///
/// v35: a jug opens. `ContainerKind` grew `Vessel` and `ClientMessage`
/// grew `TakeFromJug`, and bincode sends both enums by index: a v34
/// client shown a set-down jug's contents fails to decode the
/// `ChestState` and never opens the screen, and a v34 server handed a
/// `TakeFromJug` reads a `Respawn`. The quiet half is a rule rather than
/// a message: `BLOCK_JUG` is a container now, so a v34 client right
/// clicking a jug on a table sets a block on top of it instead of opening
/// it, and a v34 server breaking one would spill its grain loose rather
/// than fold it back into the jug. A refused handshake is the only place
/// either can be caught.
///
/// v36: the fire has a temperature. [`HearthState`] grew three fields
/// (how hot it is, how hot its batch needs it, whether the rain is on
/// it), and bincode writes fields by position: a v35 client reading a
/// v36 hearth takes the temperature for whatever comes next in the stream
/// and fails to decode the message, so the fire never opens. A number of
/// its own rather than a share of v35, because the two changes were made
/// side by side and a build holding one without the other is a build
/// that would shake hands with one holding both.
///
/// v37: sitting and lying are seen. [`PlayerState`] grew a posture byte
/// after the outfit, `ServerMessage` grew `Posture` and `ClientMessage`
/// grew `StandUp`. A v36 client reading a v37 snapshot takes the posture
/// for the next player's id and scrambles everyone near it; a v36 server
/// handed a `StandUp` reads whatever variant sits at that index. And a bed
/// is two cells now (`types::BED_HEAD`), which a v36 client would draw as
/// two whole beds end to end.
///
/// v38: the chair. A block id (`types::BLOCK_CHAIR`) that a v37 client
/// would draw as the unknown-block placeholder and a v37 server would
/// refuse to let anybody place, and a quiet change to what a snapshot
/// means: the yaw of a player sitting in a chair is the chair's facing,
/// not their camera's, so a v37 client would draw the seat and the figure
/// facing different ways without either side saying anything was wrong.
///
/// v39: the way back and the book. `ServerMessage` grew `Discovered` (the
/// kinds of block this player has held, which is what the recipe book is
/// drawn from) and `Landmarks` (the spawn and the bags this player died
/// away from). Both were added before `Error`, and bincode sends a variant
/// as its index, so a v38 client handed a v39 `Error` reads a `Landmarks`
/// and fails the frame -- which is the safe half. The quiet half is the
/// other direction: a v39 client against a v38 server is never told what
/// it has held and draws an empty book, which looks exactly like a player
/// who has held nothing.
///
/// **Forty: the sea floor and the fish.** Twelve block ids at 280..=291
/// (kelp, seagrass, the corals, the shell and what they give) and two
/// species appended to `animals::Species` -- `Fish` and `Cod`. A v39 client
/// handed a fish reads a species index it has no variant for and fails the
/// frame; handed a stem of kelp it draws the magenta placeholder where a
/// forest is. Neither is a quiet failure, and both are why the number moved.
///
/// **Forty-one: a body has wounds, and they are dressed from the pack.**
/// `ClientMessage` grew `TreatInjury` and `ServerMessage` grew `Injuries`,
/// both sent by index -- a v40 server handed a treatment reads whatever
/// variant sits there, and a v40 client handed the mannequin's state fails
/// the frame. `Body` *lost* its `fractured` flag, which is the quieter
/// break: bincode reads fields by position, so a v40 client would take the
/// next message's first byte for a broken leg. A broken leg is now one
/// wound among the rest (see `injury`), and a flag beside the whole set
/// would be two answers to one question that could disagree.
///
/// **Forty-two: a world is laid somewhere on the planet, and it grows palms
/// and swamps.** `Welcome` grew `zone` after `preset` (`worldgen::Zone`),
/// and bincode reads fields by position: a v41 client would take the zone
/// for the spawn and build its generator at the wrong latitude -- a
/// temperate foliage tint and a temperate snowfall over a tropical server.
/// And new block ids for the palm, the coconut and the swamp, which a v41
/// client would draw as the placeholder.
///
/// **Forty-three: the raft.** `EntityKind` grew `Raft` and `EntitySource`
/// grew a fourth source; `ClientMessage` grew `UseRaft`, `Row` and `Deck`;
/// `ServerMessage` grew `Oars`. All are enums sent by index, so a v42 client
/// shown a raft fails the whole entity frame -- every animal and dropped
/// stack near a raft blinks out with it -- and a v42 server handed a `Deck`
/// reads a `Disconnect` and hangs up on a player who only stepped aboard.
/// Three block ids as well (`types::BLOCK_SAIL`, `BLOCK_OAR`, `BLOCK_RAFT`),
/// which a v42 client would draw as the placeholder in the pack.
///
/// **Forty-four: the gull.** `animals::Species` grew `Gull`, and a species
/// crosses the wire as its index in an `EntityKind::Animal`: a v43 client
/// shown a gull fails the entity frame it arrives in, which on a coast is
/// every animal and dropped stack in sight.
///
/// **Forty-five: the straw pallet is two cells, and morning does not stand
/// a sleeper up.** A straw bed's head half carries `types::BED_HEAD`, which
/// a v44 client calls an invented id -- its anti-cheat refuses it and its
/// mesher draws the placeholder at the head of every pallet. And
/// [`ServerMessage::Asleep`] now goes false at dawn while the body stays
/// `Posture::Lying`: a v44 client has no fade to lift and would show the
/// morning as a snap, which is the thing this version exists to hide.
///
/// **Forty-six: drowned wood.** Two block ids (`types::BLOCK_DROWNED_TWIG`,
/// `BLOCK_DROWNED_BOUGH`) the generator writes into every swamp pool with a
/// snag in it. A v45 client would draw each piece as the placeholder standing
/// in a hole in the pool, and its anti-cheat would call the ids invented the
/// first time the server sent a felled snag's water back.
///
/// **Forty-seven: what another player does is seen.** `PlayerState` grew a
/// [`Gesture`] after `posture`, `ClientMessage` grew `Digging` and
/// `ServerMessage` grew `Blood`. bincode reads fields by position and enum
/// variants by index, so a v46 client reads every snapshot three bytes short
/// and fails it -- nobody else is drawn at all -- and a v46 server handed a
/// `Digging` reads it as the message after it.
///
/// **Forty-eight: fires in the ground.** Eleven block ids at 440..=450 (hay,
/// the raw brick, the four stages of a pit kiln, the log pile lit and unlit,
/// the charcoal heap and the firepit), two recipes appended to `RECIPES`,
/// and `ClientMessage` grew `PileLog`. A v47 client draws every stage of a
/// kiln as the placeholder and calls the ids invented; a v47 server handed a
/// `PileLog` reads it as `Disconnect` and hangs up on a player who only laid
/// a log.
///
/// **Forty-nine: the pit kiln takes fibre.** Hay (440) is gone and the
/// kiln's middle stage is `pit_kiln_fibre`; a v48 peer would hand over or
/// expect an id this build no longer has.
///
/// **Fifty: the wild plants and the birch's bark.** Eleven block ids at
/// 470..=480 (four tall plants, four low, two picked states and the sundew), a
/// tall plant's two variant bits (`types::PLANT_TOP`, `PLANT_YOUNG`), and the
/// birch's pieces in the twig's and bough's upper steps
/// (`types::birch_branch`). A v49 client draws every plant as the placeholder,
/// calls a birch's pieces invented ids the first time a birch is felled, and
/// cannot craft the plantain poultice it has no row for.
///
/// **Fifty-one: a world is drawn at a scale.** `Welcome` grew `scale` after
/// `zone` (`worldgen::Scale`), and bincode reads fields by position: a v50
/// client would take the scale's byte for the first byte of the spawn, and
/// build its generator at the regional scale over a server drawing the
/// Earth's -- foliage tinted from provinces the server never made, and a
/// biome readout naming a meadow where the server grew a forest.
///
/// **Fifty-two: honest metal.** Four block ids at 240..=243 (the whetstone,
/// slag, the steel bar and stream tin), twenty recipes appended to `RECIPES`
/// and eight changed in place, and the variant bits of a tool now carry its
/// edge and, on iron, its steel (`tools`). A v51 client calls the first blunt
/// axe it is sent an invented id, draws slag as the placeholder, and offers a
/// copper smelt of two ore that the server's hearth will never start.
///
/// **Fifty-three: a client may ask whether it is an operator.**
/// `ClientMessage::AmIAnOperator` and `ServerMessage::Operator`, which the
/// give menu is drawn from (`ui::give_screen`) -- a v52 server answers a
/// message it cannot parse, and bincode reads a variant by its number, so
/// the two sides must agree on the list.
///
/// ...and the same bump carries the two block ids a death now leaves:
/// `BLOCK_CORPSE` and `BLOCK_REMAINS` at 205 and 206, in place of the
/// backpack v14 added for exactly this and for exactly this reason. The
/// number is not moved a second time because 53 is still unreleased --
/// what matters is that no *released* build ever meets an id it has no
/// row for. A v52 client would draw a dead player as the unknown-block
/// placeholder and refuse to open the one thing in the world worth
/// opening, and a v52 server would keep laying bags the new client draws
/// as luggage; neither is something a message layout can express, and the
/// handshake is the only place either is caught.
///
/// **Fifty-four: the sail has an angle.** `EntityKind::Raft` grew
/// `sail_angle` after `sail`, and `ClientMessage` grew [`Trim`]. bincode
/// reads fields by position and variants by index, so a v53 client takes
/// the angle's first byte for the stroke and fails the whole entity frame
/// -- every animal and dropped stack near a raft blinks out with the raft
/// -- and a v53 server handed a `Trim` reads it as the message that used to
/// sit at that index and hangs up on a player who only pulled on a rope.
///
/// The number moves this time even though 53 was never released, because
/// unlike the ids 53 carried, this changes the *layout* of a message two
/// live builds in this workspace already exchange.
///
/// **Fifty-five: a body wears what is in it, and the dead lie down.**
/// `ServerMessage::BodyWorn` is appended after `Error`, so a v54 client
/// reads its variant number as nothing it knows and fails the frame -- the
/// handshake is where that has to be caught. `Posture::Fallen` rides the
/// existing byte and would read as standing on a v54 client, which is the
/// statue this bump exists to retire; it needs no bump of its own.
///
/// **Fifty-six: fire.** `ServerMessage::PitPottery` and `Smoke` are appended
/// after `BodyWorn`, so a v55 client fails the frame the first time it sees
/// a pit or a smoky room. The same bump carries the ids fire leaves and the
/// standing torch (505..=511) and resin back at 137, and soot in the variant
/// field of boards, cobble and brick (`wildfire::soot`): a v55 client calls
/// the first sooted ceiling an invented id.
///
/// ...and the fir and the saxaul, the woods of the taiga and the desert
/// (`wood`, ids 490..=497), which a v55 client would draw as the unknown
/// block across every taiga it walks into. Carried by 56 rather than a bump
/// of their own because 56 is not released: no released build meets them.
///
/// ...and fishing on the same terms: three ids at 235..=237 (the trap, the
/// rod, the copper hook), a trap's catch in its variant (`types::trap_catch`)
/// and three recipes appended to `RECIPES`. No message changed -- a cast is a
/// `UseBlock` at water and the float is the client's own drawing
/// (`logic::fishing` in the client) -- so nothing here is a layout a v55 peer
/// would misread; what it would misread is the ids.
///
/// ...and wild bees on the same terms: three ids at 248..=250 (the hive,
/// honey, beeswax), a hive's honey in its variant (`bees::honey_in`) and two
/// recipes appended to `RECIPES`. The stings travel as health, which every
/// build already reads.
///
/// ...and the ground on the same terms (`ground`, `wood`): ids 512..=616 --
/// ten rocks, their rubble, ten soils, ten grasses, the pine and the willow,
/// each wood's own twig and bough, moss -- moss in the third bit of a log's,
/// a rock's and a cobble's variant (`ground::MOSSY`), and seven recipes
/// appended to `RECIPES`. No message changed.
///
/// ...and things set down by hand: `ClientMessage::SetDown` before
/// `Disconnect`, `ServerMessage::SetDownItem` after `Smoke`, and the id they
/// write at 1000 (`types::BLOCK_SET_DOWN`). A v55 server reads the first as
/// `Disconnect`, which is the case the handshake exists to catch.
///
/// **Fifty-seven: a position is an `f64`.** Every place in the world that
/// crosses the wire -- a player in a snapshot, an entity, the client's own
/// transform and deck, the spawn, a correction, a respawn, a posture's seat
/// and a drop of blood -- was an `f32`, and an `f32` a million blocks from
/// zero has neighbours a sixteenth of a block apart: a player seen walking
/// out there moved in sixteenths, and at ten million in whole blocks. bincode
/// writes an `f64` in eight bytes where it wrote four, so a v56 peer reads
/// every one of these messages from the wrong offset.
///
/// ...and comfort on the same number: `ServerMessage::Body` grew `recovery`,
/// and a pat of dung is id 700 (`types::BLOCK_DUNG`). Carried by 57 rather
/// than a bump of its own because 57 is not released: no released build
/// meets either.
/// ...and the two station screens: `OpenStation`, `StationBegin`,
/// `StationRun` and `CloseStation` out, `StationOpen`, `StationBegun` and
/// `StationResult` back. Appended to both enums, so an older peer would read
/// every earlier message correctly and choke on these -- which is what a
/// version bump is for. Carried by 57 for the reason the rest of 57 is: no
/// released build has ever spoken it.
/// ...and fishing, which is the same story a third time: `CastLine`,
/// `Strike`, `Reel` and `ReelIn` out, `Line` back. Appended to both enums
/// and carried by 57, which no release speaks.
/// ...and two bytes of body language: [`PlayerState::limp`] and
/// [`EntityKind::Animal`]'s [`Attitude`]. Both are one byte in a struct that
/// already carries several, both read as "nothing unusual" when zero, and
/// both are carried by 57 for the reason the rest of 57 is.
/// ...and keeping animals: `TendAnimal` out, appended, and a bowl of milk at
/// id 694 (`types::BLOCK_BOWL_MILK`). 57 still, which no release speaks.
pub const PROTOCOL_VERSION: u32 = 57;

/// What kind of container a screen is showing.
///
/// A chest is forty slots and nothing else. A hearth is seven with roles
/// -- see `crate::hearth` -- and a fire under them; a drying rack is two
/// with roles and the weather over them -- see `crate::rack`. The client
/// draws three different screens and the server serves one set of
/// gestures, which is exactly the split this enum exists to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerKind {
    Chest,
    Hearth(crate::hearth::Kind),
    Rack,
    /// A jug set down on something: one slot of loose goods, up to
    /// `inventory::JUG_UNITS`. See `types::opens_as_vessel`.
    Vessel,
}

/// What a hearth is doing, for the screen that is watching it.
///
/// Seconds rather than fractions for the fuel, because the screen shows
/// a flame that shrinks and a player wants to know whether it will last
/// the batch; a fraction of a load that is itself of unknown size says
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HearthState {
    /// Seconds of fuel left, or zero if it is out.
    pub fuel_left: f32,
    /// How far through the current batch, 0..1. Zero when nothing is
    /// cooking, which is also what an unlit hearth reports.
    pub progress: f32,
    /// How hot the fire is, in degrees Celsius. Not zero just because it
    /// is out: a hearth that has burnt through its fuel is still hot for
    /// a minute, and still cooking what it is hot enough for.
    pub degrees: f32,
    /// How hot the batch in it needs the fire to be, or zero if there is
    /// no batch. **This is the number the screen exists to show beside
    /// the other one**: a fire that is burning and not working has, now,
    /// a third reason -- it is not hot enough for what is in it -- and
    /// that reason is the difference between feeding it wood and feeding
    /// it charcoal. See `crate::hearth::needs_degrees`.
    pub needs: f32,
    /// Whether rain is falling on it. A wet fire is a cooler fire (see
    /// `logic::fire` on the server), and a player looking at a gauge that
    /// will not climb should be told why rather than left to wonder what
    /// is wrong with their charcoal.
    pub wet: bool,
}

/// What a drying rack is doing, for the screen that is watching it.
///
/// **The facts rather than the wording.** A rack that is not moving has
/// three different reasons -- there is nothing on it, it is raining on
/// it, it is below freezing -- and they are three different sentences in
/// four languages. What goes on the wire is what the *weather* is doing;
/// which sentence that is belongs to the client, which is where every
/// other word on the screen is decided.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RackState {
    /// How far through the skin on the frame, 0..1. Zero for an empty
    /// rack, which is also what a rack whose tray is full reports.
    pub progress: f32,
    /// How fast it is going, as a multiple of ideal weather. Zero is
    /// stopped; the screen turns it and `rack::CURE_SECONDS` into the
    /// minutes a player actually wants.
    pub rate: f32,
    /// Rain is falling on it. Its own fact rather than "the rate is
    /// zero", because a wet rack and a frozen one are stopped for
    /// different reasons and a player can only do something about one
    /// of them.
    pub wet: bool,
    /// There is a fire beside it -- the oldest way of curing a skin
    /// there is, and the thing that makes a tundra workable.
    pub near_fire: bool,
}

/// What is extending a server, as facts rather than as sentences.
///
/// ## Why this is not the text `/mods` prints
///
/// The command has always answered the same question, and it answers it
/// by formatting: `"greeter 0.2.0 (API 2.1) -- says hello"`. That is the
/// right shape for a console and the wrong one for a screen. A client
/// given those lines would have to parse them back apart to put a
/// version in one column and a description in another, and it would
/// have to do it again in a language the server does not know it is
/// being read in. Two renderings of one fact, one of which is a parser,
/// is exactly the arrangement that drifts.
///
/// So the wire carries the columns and the client decides the wording.
/// `/mods` keeps its lines -- an operator at a console has no screen --
/// and both are built from the same host state.
///
/// ## What a client may do with it
///
/// **Read it.** There is no message for turning a mod off or changing a
/// setting from here, and that is a decision rather than a gap. A mod's
/// settings live in its own `mod.ron` on the server's disk; a client
/// that could write them would be a client writing files on somebody
/// else's machine, and applying them would mean unloading and reloading
/// native code under a running world. An operator who wants a different
/// setting edits the manifest and restarts, which is the only version of
/// that operation this game can honestly offer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtensionList {
    /// The native mod contract this server was built against, or `None`
    /// where the build has no native loader at all.
    ///
    /// An `Option` rather than a zero, because "this server runs mods
    /// and has none installed" and "this build cannot run mods" are
    /// different answers and a player deserves the one that is true.
    /// The client's own embedded server is always the second: it is
    /// built with `default-features = false` precisely so a
    /// singleplayer world carries no scripting engine and `dlopen`s
    /// nothing.
    pub native_api: Option<(u16, u16)>,
    /// Whether this build can run scripted plugins, on the same terms.
    pub scripts_supported: bool,
    /// Everything loaded, scripts and native mods together, in load
    /// order.
    pub items: Vec<ExtensionInfo>,
}

/// Which of the two extension points something came through.
///
/// The screen says so, because the two differ in the one way a player
/// cares about: a scripted plugin that goes wrong is a log line and a
/// native mod that goes wrong is the whole process. See
/// `primitive_modapi`'s module note for the rest of that table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionKind {
    /// A scripted plugin: no build step, sandboxed, interpreted.
    Script,
    /// A compiled library loaded into the server process.
    Native,
}

/// One loaded extension, as a screen needs it.
///
/// Every field here is declared by the extension itself, in its manifest
/// -- `plugin.toml` for a script and `mod.ron` for a native mod -- and
/// **not through the native ABI**. That is worth stating because it is
/// what kept `primitive_modapi` untouched by this screen: the host reads
/// the manifest before it `dlopen`s anything, so a mod's name, version,
/// authors, description and settings are all known without asking the
/// library a single question. Adding an out-struct to the contract to
/// carry them would have frozen a shape nothing needed frozen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionInfo {
    pub kind: ExtensionKind,
    pub name: String,
    /// The extension's own version, as it wrote it. Empty is possible:
    /// a plugin manifest may leave it out.
    pub version: String,
    pub description: String,
    /// Who wrote it. A list rather than a string, because a mod manifest
    /// carries a list and joining it on the server would be the server
    /// choosing a separator for a screen it cannot see.
    pub authors: Vec<String>,
    /// Whether it is actually running. False for one switched off in its
    /// manifest, and for one the host stopped calling after too many
    /// failures -- see `reason`.
    pub enabled: bool,
    /// Why it is not running, when that is not obvious. Empty when it
    /// is: a mod that is simply loaded and working has nothing to say
    /// here, and a screen that prints "ok" on every row has taught the
    /// eye to skip the column that matters.
    pub reason: String,
    /// The contract version a native mod was built against. `None` for a
    /// script, which is not built against one.
    pub built_for: Option<(u16, u16)>,
    /// The settings the manifest declares, already rendered, in the
    /// manifest's own order.
    ///
    /// Rendered on the server because the values are the *mod's* types
    /// -- RON carries tagged values the host deliberately does not
    /// understand (see `primitive_modapi::manifest`) -- and a client
    /// that received them raw would need a copy of that vocabulary to
    /// print a number.
    pub settings: Vec<(String, String)>,
}

pub const MAX_USERNAME_LEN: usize = 24;
pub const MAX_CHAT_LEN: usize = 256;

/// What a player is wearing and what is in their hand.
///
/// ## Why this rides in the snapshot instead of a message of its own
///
/// Everything else the server owns about a body -- health, hunger, the
/// pack, the worn set itself (`EquipmentState`) -- is sent *on change*,
/// which is the right shape for a number only its owner can see. This
/// is the first thing about a player that other people look at, and
/// that changes what "on change" costs: a message sent when a helmet
/// goes on reaches whoever is standing there at that moment, and
/// everybody who walks up afterwards sees a bare head until the next
/// time it changes. Getting that right means the server remembering
/// which of its players has been told about which other player's
/// sleeves, and re-sending on every entry into an interest radius --
/// a per-pair table, kept correct across joins, deaths, teleports and
/// the radius edge, to save some bytes.
///
/// So it rides in the snapshot, which is already the complete truth
/// about everyone nearby, once a tick. A player who puts a helmet on is
/// seen in it on the next tick by everyone who can see them, with no
/// bookkeeping anywhere, and a dropped snapshot corrects itself fifty
/// milliseconds later.
///
/// ## What it costs
///
/// Ten bytes: four garments and one held block, at two bytes each,
/// with no length prefix because the array is a fixed size (a `Vec`
/// would spend eight more bytes per player per tick on a length both
/// ends already know). At the default twenty ticks a second that is
/// **200 bytes per second, per visible player, per recipient** -- so
/// two people standing together cost 400 B/s between them, and the
/// densest case the interest radius allows, sixteen players in sight of
/// each other, costs 48 kB/s across the whole server. Against the
/// 32 kB a *single* chunk costs to stream, that is not a number worth
/// designing around.
///
/// ## Block ids rather than anything cleverer
///
/// Four garments out of twelve and one held block out of a couple of
/// hundred would fit in far less. It is not worth it: an index into the
/// garment table is a second numbering that both sides have to agree
/// on, and the day somebody adds a garment it is a numbering that can
/// disagree *silently* -- a helmet drawn as boots. A block id is the
/// name the rest of the game already uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Outfit {
    /// Indexed by `equipment::Slot::index`. `BLOCK_AIR` for an empty
    /// slot, which is how the rest of the game already says "nothing"
    /// -- an `Option<BlockId>` would be a tag byte per slot per tick to
    /// express what a zero expresses for free.
    pub worn: [BlockId; crate::equipment::SLOTS],
    /// What is in the selected hotbar slot, or `BLOCK_AIR` for an empty
    /// hand.
    pub holding: BlockId,
}

impl Outfit {
    /// Nothing on and nothing in hand.
    pub const BARE: Outfit = Outfit {
        worn: [crate::types::BLOCK_AIR; crate::equipment::SLOTS],
        holding: crate::types::BLOCK_AIR,
    };

    /// What is on one body part, or `BLOCK_AIR`.
    ///
    /// Indexed rather than matched, so a fifth slot is a row in
    /// `equipment::ALL_SLOTS` and nothing here.
    #[inline]
    pub fn worn_in(&self, slot: crate::equipment::Slot) -> BlockId {
        self.worn
            .get(slot.index())
            .copied()
            .unwrap_or(crate::types::BLOCK_AIR)
    }

    /// Whether there is anything to draw at all.
    ///
    /// The common case on a fresh world, and what lets the client skip
    /// the whole overlay pass for a player wearing nothing.
    #[inline]
    pub fn is_bare(&self) -> bool {
        *self == Outfit::BARE
    }
}

/// One player's state as sampled by the server on a given tick.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PlayerState {
    pub id: PlayerId,
    /// Feet, in world space. `f64` for the reason every position on the
    /// wire is: see `PROTOCOL_VERSION`'s fifty-seven.
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    /// What they look like: see [`Outfit`], which also carries the
    /// argument for why this is here rather than in a message of its
    /// own.
    pub outfit: Outfit,
    /// Standing, sitting or lying down. See [`Posture`].
    pub posture: Posture,
    /// What their hands are doing. See [`Gesture`].
    pub gesture: Gesture,
    /// How badly they are limping: 0 for a sound walk, 255 for the worst.
    ///
    /// **A broken leg was a number in the walker's own client and nothing
    /// anybody else could see.** `injury::Injuries::speed_factor` has slowed a
    /// fractured leg since fractures existed, and `survival` has had a walk
    /// that costs more when there is nothing left to spend it out of -- so on
    /// another screen a player who had just fallen down a shaft walked home at
    /// two thirds the pace with a perfectly even stride, which reads as
    /// somebody strolling rather than as somebody hurt. The pace crossed the
    /// wire (it is in the positions) and the *gait* did not.
    ///
    /// **One byte, and a scale rather than a flag**, for the reason
    /// [`Posture`] is a byte: a `bool` cannot tell a twisted ankle from a
    /// femur, and the interesting half of an injury is the part where somebody
    /// is still walking. Zero is a sound player, which is nearly every player
    /// nearly always, and a client that has never heard of the field reads a
    /// zero and draws what it drew before.
    ///
    /// Rejected: **sending the injuries themselves.** Which bone is broken is
    /// the hurt player's own business -- it is their screen that shows the
    /// body -- and a watcher only ever needs the one number the body language
    /// is made of. See `survival::limp` for where it comes from.
    pub limp: u8,
    /// **Which leg it is: the left one when true.**
    ///
    /// The limp was a scale and nothing else, and the figure on another screen
    /// always favoured its right leg -- so a player who had broken their left
    /// was watched limping on the wrong one, by the one person who had seen
    /// them fall. A `bool` beside the byte rather than a sign on it, because
    /// the byte is a severity everywhere it is read and a severity that can be
    /// negative is a clamp somebody forgets. Only a fracture has a side
    /// (`injury::Injuries::broken_leg`); exhaustion and a failing body favour
    /// the right, as every limp did before.
    ///
    /// Rejected, again: sending the injuries themselves -- which bone is
    /// broken is still the hurt player's own business, and "the left leg is
    /// the bad one" is a thing anybody watching can see.
    pub limp_left: bool,
}

/// How a player's body is arranged: on their feet, on a seat, or lying
/// down.
///
/// **On the wire because nobody could see it.** Sitting and sleeping were
/// server state and nothing more: a sleeper was drawn standing upright in
/// the middle of their bed on every other screen, and a player on a stool
/// stood beside it. What another player looks like is the snapshot's job
/// (see `Outfit` for that argument), so this rides in `PlayerState`, and
/// for a sleeper the snapshot's `yaw` is the way the bed runs.
///
/// **One byte, not an enum tag.** bincode writes an enum's tag as a
/// `u32`, and three values in four bytes per player per tick is the waste
/// `Outfit` was designed around. An unknown byte reads as standing, the
/// one reading that cannot trap a figure in a pose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub enum Posture {
    #[default]
    Standing,
    Sitting,
    Lying,
    /// Dead, and not yet respawned.
    ///
    /// **A dead player was a statue.** The server keeps a dead player in
    /// every snapshot until they press respawn -- their position is still
    /// theirs, and dropping them would read as a disconnect -- and nothing
    /// in the snapshot said they were dead, so on every other screen the
    /// figure went on standing over its own body. Only ever in
    /// `PlayerState`: the player's own `ServerMessage::Posture` never
    /// carries it, because the death screen is how a client learns it died.
    Fallen,
    /// In water past the waist: swimming, face down, rather than walking.
    ///
    /// **A swimmer was drawn walking on the lake bed.** The player's own
    /// physics has swum since water had a depth (`physics::Player::swimming`),
    /// and nothing told anybody else: on every other screen the figure stood
    /// upright in the water and strode through it, its legs pedalling at the
    /// pace of the stroke. Judged by the server against its own world at the
    /// waist (`waist_in_water`), the same line the client's physics swims at,
    /// so what everybody sees is what the swimmer's own body is doing.
    ///
    /// Only ever in `PlayerState`, like `Fallen`: a client knows it is
    /// swimming without being told, and the posture message is about
    /// furniture. The byte after `Fallen`, so a v56 client that is sent one
    /// draws a swimmer standing, which is what it drew already.
    Swimming,
}

impl From<u8> for Posture {
    fn from(byte: u8) -> Self {
        match byte {
            1 => Posture::Sitting,
            2 => Posture::Lying,
            3 => Posture::Fallen,
            4 => Posture::Swimming,
            _ => Posture::Standing,
        }
    }
}

impl From<Posture> for u8 {
    fn from(posture: Posture) -> u8 {
        posture as u8
    }
}

/// What a player's hands are doing, for everybody who can see them.
///
/// **On the wire because a player breaking a block was a statue.** The server
/// judged every swing, every edit and every mouthful and told nobody, so on
/// another screen a miner stood still until the block beside them vanished, a
/// spear was never thrust and a meal was never eaten ("анимаций игроков мало,
/// не видно как игрок ломает"). The model could draw a blow and nothing said
/// when.
///
/// **A state and a counter, not an event.** Digging lasts, so it is a flag the
/// snapshot carries for as long as it is true. A blow, a block set down and a
/// mouthful happen once, and a flag for those is missed by every client whose
/// snapshot falls either side of the tick it was set on -- so the last one is
/// named, a count of them rides beside it, and a client that sees the count
/// change starts that gesture from its beginning. A dropped snapshot costs
/// nothing, because the next one carries the same count. Two blows between two
/// snapshots are drawn as one: that is two blows inside a twentieth of a
/// second, which nothing in the game swings.
///
/// Rejected: **an event message per gesture**, sent to everybody near. It is
/// the bookkeeping `Outfit` turned down -- who is near enough now, and who
/// walks up a moment later -- for something that only matters while it is on
/// screen. Rejected too: **inventing the swing on the watching client** from a
/// block changing near somebody, which is a figure flailing at things other
/// people did.
///
/// Three bytes a player a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Gesture {
    /// Swinging at a block, for as long as they are -- what their client
    /// last said with `ClientMessage::Digging`, which the server lets lapse
    /// if it is not said again.
    ///
    /// **Read as "working what is in the hand"**, which with a fishing rod in
    /// it is winding up a cast: the watcher draws the rod back rather than
    /// chopping with it. See [`Action::Cast`] for the throw that ends it.
    pub digging: bool,
    /// The last thing they did that happens once.
    pub last: Action,
    /// How many of those there have been, wrapping. A change is a new one.
    pub count: u8,
}

impl Gesture {
    /// One more of `action`.
    pub fn made(&mut self, action: Action) {
        self.last = action;
        self.count = self.count.wrapping_add(1);
    }
}

/// A gesture that happens once. See [`Gesture`].
///
/// **One byte**, for the reason [`Posture`] is one; an unknown byte reads as
/// nothing, the one reading that cannot leave an arm in the air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub enum Action {
    #[default]
    Nothing,
    /// A blow at a player, an animal or a raft: a swing, or with a spear in
    /// hand a thrust.
    Strike,
    /// A block set down.
    Place,
    /// Something eaten.
    Eat,
    /// A jug, a coconut or a river drunk from.
    Drink,
    /// A line thrown: the rod whipped forward off the wind-up.
    ///
    /// The wind-up before it rides [`Gesture::digging`], which to a watcher
    /// is "working what is in the hand" -- with a rod in it, drawing the rod
    /// back. Appended, so it rides fifty-seven's bump: an older client reads
    /// the byte as nothing, which is a cast with no arm, what it drew before.
    Cast,
}

impl From<u8> for Action {
    fn from(byte: u8) -> Self {
        match byte {
            1 => Action::Strike,
            2 => Action::Place,
            3 => Action::Eat,
            4 => Action::Drink,
            5 => Action::Cast,
            _ => Action::Nothing,
        }
    }
}

impl From<Action> for u8 {
    fn from(action: Action) -> u8 {
        action as u8
    }
}

pub type EntityId = u64;

/// Which simulation an entity id came from.
///
/// **Three simulations put entities into the same snapshot** -- falling
/// blocks, dropped stacks and animals -- and each of them counted its
/// own from one. The client keys its entity table on the id alone, so a
/// falling block and an animal that both happened to be number seven
/// were one row in that table, and whichever was written last won.
/// Animals are appended after falling blocks, so the animal always won.
///
/// That is the bug a player described as sand "falling instantly, or
/// after a pause with no animation, or not at all": the block *was* in
/// the air on the server the whole way down, sending a position every
/// tick, and the client was drawing a deer at that row instead. All the
/// player ever saw was the cell going empty and, a moment later, a cell
/// further down filling in -- a teleport whose length is how far the
/// sand fell, which is exactly why one block looked fine and five did
/// not.
///
/// The cure is that an id says where it came from. The top eight bits
/// are the source and the remaining fifty-six are that simulation's own
/// count, so the three spaces cannot meet however long a server runs.
///
/// Two alternatives were rejected. **One shared counter in the server's
/// context**: correct, and it makes all three simulations unconstructible
/// without a server, which is what they are each unit-tested without
/// today. **Keying the client's table on `(kind, id)`**: also correct
/// for the three that exist, and it leaves two simulations free to
/// disagree about who owns number seven -- so the fourth thing that
/// replicates an entity brings this straight back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitySource {
    FallingBlock = 1,
    Item = 2,
    Animal = 3,
    /// See `raft`. The fourth simulation, and exactly the case the note
    /// above was written for: without a source of its own, raft number one
    /// and falling block number one would be one row on the client.
    Raft = 4,
}

/// How many low bits of an [`EntityId`] belong to the source's own
/// counter. Fifty-six of them: a server would have to drop a hundred
/// million blocks a second for two thousand years to run out.
const ENTITY_ORDINAL_BITS: u32 = 56;

/// The id under which a simulation's `ordinal`th entity is replicated.
///
/// `ordinal` is whatever the simulation counts internally, from one. It
/// is masked rather than checked: an id that wrapped would collide with
/// the same source's own oldest entity, which is a duplicate id inside
/// one simulation and nothing the others can see.
#[inline]
pub fn entity_id(source: EntitySource, ordinal: u64) -> EntityId {
    ((source as u64) << ENTITY_ORDINAL_BITS) | (ordinal & ((1u64 << ENTITY_ORDINAL_BITS) - 1))
}

/// Which simulation issued this id, if any did.
///
/// For tests and for a log line that has to say what it is looking at;
/// nothing in the game logic asks, because the kind travels in
/// [`EntityKind`] where the renderer can act on it.
#[inline]
pub fn entity_source(id: EntityId) -> Option<EntitySource> {
    match id >> ENTITY_ORDINAL_BITS {
        1 => Some(EntitySource::FallingBlock),
        2 => Some(EntitySource::Item),
        3 => Some(EntitySource::Animal),
        4 => Some(EntitySource::Raft),
        _ => None,
    }
}

/// What an animal is doing, as far as its body shows it.
///
/// **The server has always known and never said.** An animal's mind is a
/// dozen states -- grazing, at the water with its head down, wary and looking
/// about, stalking something, dozing through the small hours -- and what
/// crossed the wire was a position and a facing, so on screen every one of
/// them was the same box walking or the same box standing. A deer that had
/// stopped at a river for four seconds and a deer that had stopped for no
/// reason were the same picture, which is why the drinking was invisible
/// enough to need a paragraph of its own in `server::logic::animals` to
/// explain how a player was supposed to infer it.
///
/// **A posture, not the mind.** The server's `Mind` has fifteen-odd values,
/// several of which are about *why* rather than about what a body looks like,
/// and half of them change twice a second. What is sent is the handful an eye
/// tells apart at thirty blocks: head down, head up, crouched, asleep. A
/// client cannot tell a deer fleeing a wolf from a deer fleeing a player and
/// does not need to -- it can see both running.
///
/// **One byte, for the reason [`Posture`] is one**, and an unknown byte reads
/// as [`Attitude::Easy`]: the reading that cannot trap an animal with its head
/// in the ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub enum Attitude {
    /// Nothing in particular: walking, standing, running, being a box that
    /// goes about its business. Most of every animal's life, and the value a
    /// client falls back on for anything it does not recognise.
    #[default]
    Easy,
    /// Head up and looking: something was heard, or the feeding bout is
    /// between mouthfuls. The pose that makes a herd read as watchful rather
    /// than as scenery.
    Alert,
    /// Head down in the grass, eating.
    Feeding,
    /// Head down at the water's edge, lower than feeding and reaching
    /// forward: see the server's `Mind::Drink`.
    Drinking,
    /// Crouched and coming on slowly: a wolf's or a lion's stalk, before the
    /// burst. The one attitude that belongs to a predator alone.
    Stalking,
    /// Asleep on its feet, or resting out the middle of the day in the shade.
    /// Head low, and nothing moving.
    Dozing,
    /// **Dead, and going down.** The second or so between the killing blow
    /// and the carcass: the server keeps the body as an entity for
    /// `animals::FALL_SECONDS`, still moving under gravity and its last
    /// shove, and a client rolls it onto its side with the legs gone stiff
    /// (`animal_model::Motion::fallen`). Then the entity goes and the carcass
    /// block is laid where the body came to rest.
    ///
    /// **An attitude, not a message of its own**, because it is exactly what
    /// this byte is for -- what the body is doing, for everybody who can see
    /// it -- and it arrives in the same snapshot as the position the body is
    /// falling through. A `Died` event beside the snapshot would be two things
    /// that can arrive in either order. Appended, for the byte's reason: an
    /// old client reads a 6 as `Easy`, and sees an animal stand still for a
    /// second before its carcass appears, which is what it saw before.
    Dying,
}

impl From<u8> for Attitude {
    fn from(byte: u8) -> Self {
        match byte {
            1 => Attitude::Alert,
            2 => Attitude::Feeding,
            3 => Attitude::Drinking,
            4 => Attitude::Stalking,
            5 => Attitude::Dozing,
            6 => Attitude::Dying,
            _ => Attitude::Easy,
        }
    }
}

/// How long a killed animal is kept as a body going down before its carcass
/// is laid, in seconds: see [`Attitude::Dying`]. Here, beside the attitude,
/// because both sides read it -- the server keeps the body this long
/// (`animals::FALL_SECONDS`) and the client times the roll inside it
/// (`animal_model::FALL_SECONDS`) -- and two copies of one number is a roll
/// that ends after the carcass has already appeared.
pub const DEATH_FALL_SECONDS: f32 = 0.8;

impl From<Attitude> for u8 {
    fn from(attitude: Attitude) -> u8 {
        attitude as u8
    }
}

/// What an entity is. Kept as an enum rather than a free id so the
/// client can't be asked to render something it doesn't understand.
///
/// `PartialEq` and no `Eq`: an animal carries a facing and a hurt
/// flash, both of them floats, and `Eq` on a float is a promise this
/// type cannot make. Nothing compares entity kinds for anything but
/// equality in a test, so the weaker trait costs nothing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    /// A block in mid-fall. `block` is what it will become when it
    /// lands, and what it's drawn as.
    FallingBlock { block: BlockId },
    /// A dropped stack lying in the world, waiting to be picked up.
    /// Drawn as a small copy of the block it is.
    Item { block: BlockId, count: u32 },
    /// A living thing. The server decides everything about it; this is
    /// what the client needs in order to draw one.
    ///
    /// `yaw` rather than a velocity, because facing is the only part of
    /// an animal's motion the client cannot infer: position comes twice
    /// a tick and is interpolated the way every other entity's is, but a
    /// deer that has stopped is still looking somewhere, and a box that
    /// snaps to face its last direction of travel reads as a bug.
    ///
    /// `hurt` is a flash rather than a health bar -- how recently it was
    /// hit, 0..1, decaying on the server. A number the client turns into
    /// a red tint for a fraction of a second, which is the whole of the
    /// feedback a swing needs. A full health figure would invite a health
    /// bar over every animal, and a world of health bars is a world of
    /// interface rather than of animals.
    ///
    /// `attitude` is what the animal is *doing*, as far as a body says it:
    /// see [`Attitude`], which is where the argument for it lives.
    ///
    /// `growth` is how grown it is, 255 for an adult: see `youth::to_wire`.
    /// A client draws a young animal at `youth::size` of its species and aims
    /// at a box that size, which is the box the server checks a blow against.
    Animal {
        species: crate::animals::Species,
        yaw: f32,
        hurt: f32,
        attitude: Attitude,
        growth: u8,
    },
    /// A raft on the water. The entity's `y` is its waterline -- see
    /// `raft::Body`.
    ///
    /// **Velocity and turn are sent, unlike an animal's**, because two
    /// clients need them. The one rowing predicts its raft ahead of the
    /// server and has to correct toward where the server's raft *will* be
    /// when the correction lands, which is a position plus a velocity; and
    /// everyone standing on a raft is carried by it, so the deck under
    /// them has to be where the raft is going, not where the last snapshot
    /// left it. Five floats more per raft per tick, for a handful of rafts.
    ///
    /// `stroke` is what the oars are doing, so another player sees them
    /// pulling -- and sees them still when nobody is at them. `hurt` is the
    /// animal's flash: a blow on the timber.
    ///
    /// `sail_angle` is how far the yard is braced round from square
    /// (`raft::Body::sail_angle`). Sent to everyone and not only to the
    /// hand on it, because it is the one thing about a raft that says what
    /// it is *about* to do: a sail braced across the wind is a raft that
    /// is going somewhere, and everyone watching it -- and every client
    /// drawing the yard -- has to be looking at the same angle.
    Raft {
        yaw: f32,
        vx: f32,
        vz: f32,
        spin: f32,
        sail: bool,
        sail_angle: f32,
        stroke: f32,
        hurt: f32,
    },
}

/// One entity as sampled by the server on a given tick.
///
/// Entities are replicated the same way players are: a per-tick
/// snapshot of everything near the recipient, with no explicit despawn
/// message. A client drops anything it stops hearing about, which means
/// a lost despawn can't leave a permanent ghost -- the failure mode is
/// an entity lingering for a fraction of a second.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EntityState {
    pub id: EntityId,
    pub kind: EntityKind,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A single block change, for batched updates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BlockChange {
    pub global_x: i32,
    pub global_y: i32,
    pub global_z: i32,
    pub block_id: BlockId,
}

/// Why the server is disconnecting someone. Kept as an enum rather than a
/// free string so a client (or a future admin UI) can react per-reason.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DisconnectReason {
    ProtocolMismatch { server_version: u32 },
    ServerFull,
    Banned,
    Timeout,
    AntiCheat(String),
    RateLimited,
    ServerShutdown,
    Other(String),
}

impl std::fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisconnectReason::ProtocolMismatch { server_version } => {
                write!(f, "protocol mismatch (server speaks v{server_version})")
            }
            DisconnectReason::ServerFull => write!(f, "server is full"),
            DisconnectReason::Banned => write!(f, "banned"),
            DisconnectReason::Timeout => write!(f, "timed out"),
            DisconnectReason::AntiCheat(d) => write!(f, "anti-cheat: {d}"),
            DisconnectReason::RateLimited => write!(f, "too many requests"),
            DisconnectReason::ServerShutdown => write!(f, "server shutting down"),
            DisconnectReason::Other(d) => write!(f, "{d}"),
        }
    }
}

/// Which of the two open inventories a chest gesture is about.
///
/// A side and a slot rather than one number over both, because the two
/// inventories are two different things on the server -- one belongs to
/// the player and one to a place in the world -- and a single index
/// space would mean a client could name a chest slot where a pack slot
/// was expected by getting the arithmetic wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    /// The player's own pack, hotbar included.
    Pack,
    /// The chest they currently have open.
    Chest,
}

/// Messages the client sends to the server. The server is the sole source
/// of truth (авторитативный сервер) -- the client only ever *requests*
/// things, it never asserts world state directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Must be the first message on the connection.
    Hello {
        protocol_version: u32,
        username: String,
    },
    RequestChunk(ChunkPos),
    /// Batched form of `RequestChunk` -- one message for a whole new
    /// render-distance ring instead of ~50.
    RequestChunks(Vec<ChunkPos>),
    SetBlock {
        global_x: i32,
        global_y: i32,
        global_z: i32,
        block_id: BlockId,
    },
    /// "My feet are here, looking this way." Rate-limited client-side and
    /// re-validated server-side (see the server's `anticheat` module --
    /// this is exactly the message a cheat client would lie in).
    UpdateTransform {
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
        /// Monotonic per-client counter, so the server can detect
        /// reordering/replay and measure the real update rate.
        sequence: u32,
    },
    Chat(String),
    /// "Am I an operator?"
    ///
    /// **Asked because a menu has to be drawn before it is used.** The
    /// give menu (`ui::give_screen`) is the operator's command `/give`
    /// with a page in front of it, and a page that opens for everybody
    /// and then refuses everybody is a page that reads as broken. The
    /// client cannot work the answer out for itself: who is an operator
    /// is the server's profiles and its own `local_operator`, neither of
    /// which is on the wire.
    ///
    /// Asked again every time the journal is opened rather than once at
    /// the handshake, because `/op` takes effect on the next command and
    /// not on the next login -- a flag settled at the door would be
    /// wrong for as long as the session lasted.
    AmIAnOperator,
    /// Reply to `ServerMessage::Ping`, echoing the nonce back.
    Pong {
        nonce: u64,
    },
    /// "Move the stack in `from` onto `to`."
    ///
    /// Merged if the two hold the same block, swapped otherwise. It was
    /// a plain swap in v5, which meant two part-stacks of stone could
    /// never be made into one.
    MoveSlots {
        from: u8,
        to: u8,
    },
    /// "Put half of `from` in `to`." `to` has to be empty or hold the
    /// same block.
    SplitSlot {
        from: u8,
        to: u8,
    },
    /// "Send this stack between the bar and the pile behind it" -- the
    /// shift-click. Which way round is decided by where the slot is, so
    /// there is nothing here to get wrong.
    QuickMoveSlot {
        slot: u8,
    },
    /// "Tidy the storage rows." The hotbar is left alone: where things
    /// sit on the bar is an arrangement the player made.
    SortInventory,
    /// "Tidy the container I have open."
    ///
    /// The same gesture as `SortInventory`, aimed at the other side of
    /// the screen. A chest is the *one* place a player accumulates
    /// forty part-stacks of nine things, and it was the one place with
    /// no way to fold them together.
    SortChest,
    /// "Throw this out." `whole_stack` is the shift-click: all of it
    /// rather than one.
    DropSlot {
        slot: u8,
        whole_stack: bool,
    },
    /// "Make recipe number `index`, up to `times` of it." An index
    /// rather than a description of the recipe, so a client can only ask
    /// for one the server also has -- it looks the index up in its own
    /// table. The server stops early when the ingredients or the room
    /// run out, so `times` is an ask rather than a promise.
    Craft {
        index: u16,
        times: u8,
    },
    /// Which hotbar slot is selected, so the server knows what a
    /// placement should spend.
    SelectSlot {
        slot: u8,
    },
    /// "Open the chest at that cell."
    ///
    /// The server checks it is within reach and that the cell really
    /// holds a chest, then answers with `ChestState` and remembers which
    /// chest this player has open. Every gesture below is against *that*
    /// chest and carries no position of its own, so a client cannot
    /// reach into a chest across the map by naming it.
    OpenChest {
        global_x: i32,
        global_y: i32,
        global_z: i32,
    },
    /// "I am done with it." Also sent when the screen closes for any
    /// other reason, so the server stops sending updates for it.
    CloseChest,
    /// "Move this slot onto that one." `half` is the right-click: half
    /// the stack rather than all of it.
    ///
    /// Both sides may be the pack or the chest, so this one message is
    /// also how things are rearranged *within* an open chest.
    ChestMove {
        from: (Side, u8),
        to: (Side, u8),
        half: bool,
    },
    /// "Send this slot to the other side" -- the shift-click. Which way
    /// round is decided by which side it is on, so there is nothing here
    /// to get wrong.
    /// Everything that fits, in one gesture.
    ///
    /// One message rather than forty `ChestQuickMove`s, for two
    /// reasons: a burst of forty is exactly what the message rate limit
    /// exists to stop, and a bulk transfer that is *partly* applied
    /// because the fortieth was dropped is worse than one that is not
    /// applied at all.
    ChestBulkMove {
        /// True to send the pack into the chest, false to empty the
        /// chest into the pack.
        to_chest: bool,
    },
    ChestQuickMove {
        side: Side,
        slot: u8,
    },
    /// "I swung at that player."
    ///
    /// Deliberately the whole of the message. No damage figure, no
    /// position, no direction: the server has its own copy of where
    /// everyone is and its own idea of what a punch is worth, and the
    /// only thing it cannot work out for itself is who was aimed at.
    /// See `primitive_shared::combat` for what it checks.
    Attack {
        target: PlayerId,
    },
    /// The same swing, aimed at something that is not a player.
    ///
    /// A separate message rather than a widened `Attack`, because the
    /// two id spaces are separate: player ids and entity ids are both
    /// `u64` and mean different things, and one message carrying either
    /// would be a message whose meaning depends on which of two tables
    /// the number happens to be found in. That is exactly the kind of
    /// ambiguity an anti-cheat has to resolve, and it should not have to.
    AttackEntity {
        target: EntityId,
    },
    /// "I am swinging at a block that is coming apart" -- or "I have
    /// stopped".
    ///
    /// **What the arm is doing, and nothing it decides.** Breaking is still
    /// `SetBlock` at the end of the swing, judged by the server as it always
    /// was; this changes a figure on other people's screens and nothing in the
    /// world, so there is nothing in it to cheat with. Sent when it changes
    /// and every half second while it is true, and the server lets it lapse
    /// (`players::DIGGING_LAPSES_AFTER`), so a client that drops mid-swing does
    /// not leave a figure hammering at nothing. See [`Gesture`].
    ///
    /// Rejected: working it out on the server from the edits it accepts.
    /// Iron ore is thirteen seconds of swinging and one edit, so the arm would
    /// move once, at the end, which is the statue this exists to cure.
    Digging {
        digging: bool,
    },
    /// "Eat what is in that slot."
    ///
    /// A slot rather than a block id, for the reason `DropSlot` takes
    /// one: the server's copy of the pack is the real one, and a client
    /// naming a *block* would be a client asking to eat something it
    /// might not have. The server checks the slot holds food and that
    /// eating it would do anything at all -- a haunch spent on a full
    /// stomach is an item destroyed.
    Eat {
        slot: u8,
    },
    /// "Put what is in this slot of my pack on that part of my body."
    ///
    /// The gesture the mannequin exists for: a bandage picked up out of the
    /// pack and dropped on a bleeding arm. **A slot and a part, and nothing
    /// about the wound**, on the rule `Eat` follows -- the server reads the
    /// item out of its own copy of the pack and the wound out of its own
    /// copy of the body, so a client can neither dress a wound it does not
    /// have nor spend a bandage it was never given. The part is an index
    /// into `injury::Part::ALL`; out of range is silence.
    ///
    /// Whether the item suits the wound is the server's to decide (see
    /// `injury::Injuries::treat`), and a refusal keeps the item and says why.
    TreatInjury {
        slot: u8,
        part: u8,
    },
    /// "I right-clicked that block, and I did not mean to place
    /// anything."
    ///
    /// The gesture that was missing. A right click used to be one of two
    /// things -- put a block against this face, or open the chest -- and
    /// a fire needs a third: strike a spark into it, or feed it what is
    /// in your hand. Rather than teach `SetBlock` to sometimes not set a
    /// block, this says plainly what it is.
    ///
    /// What it does is decided entirely by the server from the block at
    /// the cell and what the player is holding, so this carries neither.
    /// A client that could name the *effect* could light a fire with an
    /// empty hand.
    UseBlock {
        global_x: i32,
        global_y: i32,
        global_z: i32,
    },
    /// "Get up": off the bed, or off the stool.
    ///
    /// **What "press any key to get up" was missing.** The screen said it
    /// and nothing sent it: the only way out of a bed was a right click on
    /// that bed, from a body the client had stopped moving. A message of
    /// its own rather than a second `UseBlock`, because getting up is not
    /// about a cell -- a sleeper whose bed was just broken has none to name.
    StandUp,
    /// "Put on whatever is in this slot of my pack."
    ///
    /// A slot rather than a block, on the same rule `Eat` follows: the
    /// server's copy of the pack is the real one, and a client naming a
    /// *block* would be a client asking to wear something it might not
    /// have.
    ///
    /// Which body part it goes on is not in the message either, because
    /// it is a fact about the garment (`equipment::slot_of`) rather than
    /// a choice -- so there is nothing here for a client to get wrong or
    /// to lie about.
    Equip {
        slot: u8,
    },
    /// "Take off what is on this body part and put it in my pack."
    ///
    /// The index is into `equipment::ALL_SLOTS`. Out of range is
    /// silence, like every other out-of-range index in this protocol.
    Unequip {
        slot: u8,
    },
    /// "Pour what is in `from` into the jug in `jug`."
    ///
    /// Two slots and nothing else. Not *how much*: a pour is all of it
    /// that fits, because a player choosing a number is a second screen
    /// and the thing they actually want is a full jug. Not *what*
    /// either -- the server reads both slots out of its own copy of the
    /// pack, on the same rule `Eat` and `Equip` follow, so a client
    /// naming a block would be a client asking to pour something it
    /// might not have.
    ///
    /// The server re-checks all three of the rules the client used to
    /// offer the gesture: that `from` really holds something that pours
    /// (`types::pours`), that `jug` really holds an empty jug or one
    /// already holding the same goods, and that the result is inside
    /// `inventory::JUG_UNITS`.
    PourIntoJug {
        from: u8,
        jug: u8,
    },
    /// "Tip that jug out into my pack."
    ///
    /// Whatever will not fit stays in the jug rather than falling on the
    /// floor or ceasing to exist, so this is safe to send against a full
    /// pack and the client does not have to work out in advance whether
    /// there is room.
    EmptyJug {
        slot: u8,
    },
    /// "Take what is in the jug in `jug` out into `to`" -- all of it, or
    /// half when `half` is set.
    ///
    /// The third gesture on a jug, and the one opening it needed:
    /// `EmptyJug` tips everything into wherever the pack has room, which
    /// is right for a shift-click and wrong for a player dragging seven
    /// seeds out onto the square beside the hoe. **A slot to land in, and
    /// no count**, on the rule `PourIntoJug` states -- the server reads
    /// what is in the jug from its own copy of the pack, so a client that
    /// named a number would be naming grain it might not have. What will
    /// not fit in `to` stays in the jug.
    TakeFromJug {
        jug: u8,
        to: u8,
        half: bool,
    },
    /// "I have read the death screen, put me back in the world."
    ///
    /// Respawning is a request rather than something the server does on
    /// its own timer, so a player who died is not dropped back into the
    /// world before they have seen why.
    Respawn,
    /// "What is extending this server?"
    ///
    /// Sent when the player opens the extensions screen and never
    /// otherwise -- there is no subscription and no periodic refresh,
    /// because the answer changes when the server restarts and at no
    /// other moment. Rate-limited on the same bucket chat is, which is
    /// what stops a client asking sixty times a second for a list that
    /// costs a lock and a few dozen allocations to build.
    RequestExtensions,
    /// "I used that raft": the right click, or a tap, on its timber.
    ///
    /// Standing on the deck and away from the oars, it takes them. At the
    /// oars, it raises or furls the sail. What it does is the server's to
    /// decide from where it has the player, for the reason `UseBlock`
    /// carries no effect.
    UseRaft {
        raft: EntityId,
    },
    /// "These are my oars." Sent by the rower while they are rowing, on
    /// change and every quarter second besides, and clamped by the server
    /// (`raft::Oars::clamped`).
    ///
    /// **An input, not a position.** The server moves the raft; a client
    /// that could say where its raft was could row it through a hillside.
    /// What the rowing client does with its own keys is predict the same
    /// `raft::step` ahead of the server and be corrected -- the arrangement
    /// the player's own movement has, with the authority the other way
    /// round, because a raft carries other people and a player carries
    /// only themselves.
    Row {
        raft: EntityId,
        stroke: f32,
        turn: f32,
    },
    /// "Brace the yard round to here": the sail's angle, in radians off
    /// square, as the hand dragging it is asking for
    /// (`raft::Body::sail_angle`).
    ///
    /// **An angle and not a nudge**, unlike the oars, which are a push.
    /// A stream of "a little further round" messages would have the sail
    /// end up wherever the dropped ones left it, and the one thing the
    /// player can see about this control is exactly where it ended up. An
    /// absolute angle is idempotent: a lost message costs nothing, because
    /// the next one says the whole truth again.
    ///
    /// Clamped by the server (`raft::trim_clamped`) and refused from
    /// anybody who is not at the oars or standing by the mast
    /// (`raft::at_the_sail`) -- a player who could trim a sail from the
    /// bank would be sailing somebody else's raft off down the lake.
    Trim {
        raft: EntityId,
        angle: f32,
    },
    /// "My feet are here, on that raft's deck" -- `UpdateTransform` for a
    /// body standing on something that moves.
    ///
    /// **In the deck's own frame** (`raft::Body::local_of`), and that is the
    /// whole reason it is a message of its own. A client standing on a
    /// raft sees the raft where its last snapshot put it, a tick or two
    /// behind the server's; a world position worked out against that raft
    /// lands a stride behind the server's deck, and everyone else would see
    /// the rider sliding toward the stern of a raft going forwards. A place
    /// on the deck is the same place on both rafts, so the server puts the
    /// rider onto *its* raft and they stay where they are standing.
    ///
    /// Checked against the deck's size and against where the server had
    /// the player (`raft::BOARDING_REACH`), rather than by the anti-cheat's
    /// speed budget, which would read a rider carried at five blocks a
    /// second as a player running at five.
    Deck {
        raft: EntityId,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
        sequence: u32,
    },
    /// "Lay the log in my hand as a pile in this cell": shift and a right
    /// click with a log, TerraFirmaCraft's gesture for a log pile (see
    /// `pit`).
    ///
    /// **A message of its own rather than a flag on `UseBlock`**, because
    /// `UseBlock` names a cell that is *there* and lets the server decide
    /// what using it means; this names an empty cell and one meaning. And
    /// not `SetBlock`: a pile is not the log block the player is holding,
    /// and a placement that sometimes wrote something else would be a
    /// placement the anti-cheat and the client's prediction both have to
    /// learn exceptions to. Before `Disconnect`, which stays last so that
    /// every older message keeps its index.
    PileLog {
        global_x: i32,
        global_y: i32,
        global_z: i32,
    },
    /// "Send every stack of what is in this slot to the other side" -- the
    /// ctrl-click at an open container.
    ///
    /// **One message, for `ChestBulkMove`'s reasons**: sixteen stacks of
    /// cobble are sixteen shift-clicks, and a burst of sixteen is what the
    /// rate limit is for. The server reads what is in the slot itself and
    /// moves everything of that kind, each stack exactly as a shift-click
    /// would (into a hearth's or a rack's slots by role). Carried by 56,
    /// which is not released.
    ChestMoveKind {
        side: Side,
        slot: u8,
    },
    /// "Set one of what is in my hand down in this cell": the modifier and a
    /// right click at the top of a block, with anything that is not built
    /// with (`types::can_be_set_down`). The cell is the empty one over the
    /// face aimed at.
    ///
    /// **`PileLog`'s arrangement, for `PileLog`'s reasons**: it names an
    /// empty cell and one meaning, and what goes into the cell is not what
    /// is in the hand. No facing on the wire: the server turns it the way
    /// the player it already follows is looking (`PlayerState::yaw`), and a
    /// client that could name one could name an id it has no business
    /// writing. Carried by 56, which is not released.
    SetDown {
        global_x: i32,
        global_y: i32,
        global_z: i32,
    },
    /// "Open the anvil or the potter's wheel at that cell."
    ///
    /// The chest's arrangement exactly, and for the chest's reason: the
    /// server checks the reach and what is really in the cell, answers with
    /// `StationOpen`, and *remembers* which station this player has open.
    /// Everything below carries no position at all, so a client cannot work
    /// at an anvil across the map by naming one. See
    /// `ClientMessage::OpenChest`.
    OpenStation {
        global_x: i32,
        global_y: i32,
        global_z: i32,
    },
    /// "Start this job." The materials are taken here, and the server stamps
    /// the moment it agreed -- which is the clock rule 4 of `minigame` is
    /// judged against.
    StationBegin {
        job: crate::minigame::Job,
    },
    /// "Here is the whole run": one press per blow, in milliseconds from the
    /// moment the server let it begin.
    ///
    /// The whole run in one message rather than a message a blow, because
    /// four round trips inside five seconds is a game played against the
    /// network and is exactly the burst the rate limit exists to stop. The
    /// argument in full is in `minigame`.
    StationRun {
        presses: Vec<u32>,
    },
    /// "I am done with it." Sent when the screen closes for any reason, as
    /// `CloseChest` is.
    CloseStation,
    /// **A rod thrown.** `power` is how long the throw was held, as
    /// `fishing::cast_power` scores it (0 to 1); where it goes is the
    /// player's own look direction, which this server already has from
    /// `UpdateTransform`.
    ///
    /// **The aim is not in the message**, and that is the anti-cheat
    /// decision: a direction on the wire is a direction a client can invent,
    /// and a float placed by the server from the transform it is already
    /// validating cannot be aimed anywhere the player is not looking. The
    /// cost is that a throw made in the same frame as a flick of the mouse
    /// lands where the server last heard the player looking, which is a
    /// tenth of a second of lag on a thing that takes a second to wind up.
    CastLine {
        power: f32,
    },
    /// **The strike**: the float went under and the hand came up. Refused
    /// unless the fish is actually on the bait (`fishing::Strike`).
    Strike,
    /// **Reeling, or giving line.** Sent on the change and not every frame:
    /// the fight is stepped on the server's clock, and what the client sends
    /// is which way the hand is.
    Reel {
        pulling: bool,
    },
    /// "Take the line out of the water." A cast abandoned, with nothing on
    /// it.
    ReelIn,
    Disconnect,
    /// **One swing of a dig.** "I have worked a quarter of the way through
    /// the block at this cell, from this face."
    ///
    /// A rock or a soil comes away a slice at a time now (`dig`), and a
    /// slice is a *different* edit from a break: the cell does not become
    /// air, nothing drops, and the tool is not worn. `SetBlock` cannot say
    /// it. Asked with `block_id: BLOCK_AIR` it means the whole block, which
    /// is the one message the break path has always been, and asked with
    /// the bitten id it would arrive down the *placement* half of that
    /// path -- spending an item out of the pack to put a block that is
    /// already there back where it is.
    ///
    /// **The face and not the id.** The client says which way it is
    /// digging; the server reads what is actually in the cell and works
    /// out the next shape itself (`dig::next_bite`). A client that named
    /// the id would be a client that could name the *last* slice on the
    /// first swing, and quarry a hillside in a quarter of the time.
    ///
    /// The last slice is not one of these: when the block has a quarter
    /// left the client sends the `SetBlock` it always sent, and the break
    /// path runs whole -- the drop, the tool's wear, the collapse, the
    /// grime and the plugin hook, none of it duplicated here. See
    /// `dig::next_bite`'s `None`.
    Dig {
        global_x: i32,
        global_y: i32,
        global_z: i32,
        /// The step from the block to the empty cell the digger is on the
        /// other side of, as `dig::Side::from_normal` reads it: one of the
        /// six axial steps, and anything else is refused.
        face: (i8, i8, i8),
    },
    /// **A right click on an animal with something to tend it with**: feed
    /// held out, a knife to shear, an empty bowl to milk into
    /// (`husbandry::is_tending_tool`). Which of those it is, and whether the
    /// animal will have it, is the server's (`Animals::tend`): the client
    /// names the animal and nothing else, the way `UseRaft` names a raft.
    ///
    /// Rejected: **the swing, read differently with food in hand.** Then a
    /// knife would have to shear a tame sheep and cut a wild one, and which
    /// one a blow does would be a rule nobody could see from the hand.
    TendAnimal {
        animal: EntityId,
    },
}

/// Messages the server sends to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Handshake accepted. Carries everything the client needs to
    /// configure itself against *this* server rather than guessing:
    /// where to spawn, how far the server will actually stream, what the
    /// tick rate is, and the current time of day for lighting.
    Welcome {
        your_id: PlayerId,
        protocol_version: u32,
        server_name: String,
        tick_rate_hz: f32,
        view_distance_chunks: i32,
        world_seed: u32,
        /// Which generator made it. The seed alone is not the world --
        /// see `worldgen::Preset` -- and the client builds a generator
        /// of its own for the foliage tint and the biome readout.
        preset: crate::worldgen::Preset,
        /// Where on the planet the world is laid, for the preset's
        /// reason: the client's generator tints foliage and decides
        /// snowfall from the latitude, and the seed and the preset alone do
        /// not say which latitude that is. See `worldgen::Zone`.
        zone: crate::worldgen::Zone,
        /// Which scale the country is drawn at, for the zone's reason: the
        /// client's generator reads the climate a world of the Earth's
        /// scale has provinces in, and an old world of the same seed, preset
        /// and zone does not. See `worldgen::Scale`.
        scale: crate::worldgen::Scale,
        spawn: (f64, f64, f64),
        time_of_day: f32,
        /// The world's age in days, hour in the fraction: what the
        /// seasons are read from (`season::Season::at`). Sent beside
        /// `time_of_day` rather than instead of it because the sky wants
        /// the hour wrapped and the calendar wants it whole.
        world_days: f32,
        /// Real seconds per in-game day, so the client can keep the sun
        /// moving smoothly between `TimeSync` messages instead of
        /// stepping it every two seconds.
        day_length_seconds: f32,
    },
    /// The answer to `ClientMessage::AmIAnOperator`.
    ///
    /// A bare yes or no rather than the `Permission` enum: the client has
    /// no business holding the server's authority type, and the only
    /// question it has is whether to draw a page.
    Operator {
        yes: bool,
    },
    /// Handshake refused; the connection closes right after.
    Rejected(DisconnectReason),
    /// One chunk of the world.
    ///
    /// Behind an `Arc` because a chunk is 32 KB and the server hands the
    /// same one to every player who can see it. Sending it used to deep
    /// copy that array per player per chunk -- megabytes a second of
    /// pure memcpy while terrain streams, and in singleplayer it is the
    /// game's own process paying it. `serde`'s `rc` feature serialises
    /// an `Arc<T>` as plain `T`, so the bytes on the wire are exactly
    /// what they were.
    ChunkData(std::sync::Arc<Chunk>),
    BlockUpdate(BlockChange),
    /// Batched block changes (bulk edits, or several edits landing in the
    /// same tick).
    BlockUpdates(Vec<BlockChange>),
    /// All players near the recipient, as of `tick`. Replaces per-message
    /// relaying of movement.
    Snapshot {
        tick: u64,
        states: Vec<PlayerState>,
    },
    /// All entities near the recipient, as of `tick`. Sent only when
    /// there are any, so an idle world costs nothing.
    Entities {
        tick: u64,
        states: Vec<EntityState>,
    },
    PlayerJoined {
        id: PlayerId,
        username: String,
    },
    PlayerLeft {
        id: PlayerId,
    },
    Chat {
        from: Option<PlayerId>,
        username: String,
        text: String,
    },
    /// World clock for the day/night cycle. `time_of_day` is 0.0..1.0,
    /// where 0.0 = midnight, 0.5 = noon. Sun direction and sky/fog colour
    /// are derived from this on the client, so every player sees the same
    /// sky at the same moment.
    TimeSync {
        tick: u64,
        time_of_day: f32,
        /// See `Welcome::world_days`.
        world_days: f32,
    },
    /// Anti-cheat rubber-band: the server rejected the client's reported
    /// position and is telling it where it actually is.
    PositionCorrection {
        x: f64,
        y: f64,
        z: f64,
        reason: String,
    },
    /// Flight, granted or taken away.
    ///
    /// **The one message that changes how the client's own physics
    /// behaves**, and it is worth being explicit about why that is not
    /// a hole in the authoritative model. The client already runs
    /// gravity locally -- it has to, or every step would cost a round
    /// trip -- and the server already checks the result. This does not
    /// hand the client a new freedom: it tells the client which rules
    /// the server is *about to judge it by*, so the two sides agree
    /// before the movement happens rather than arguing about it
    /// afterwards. A client that ignored this message would simply fall.
    /// A client that granted itself flight without being told is exactly
    /// what the anti-cheat still catches.
    ///
    /// Sent on change only, and re-sent on respawn, because dying ends
    /// it -- see `logic::survival`.
    Flight {
        enabled: bool,
        /// Blocks per second, in every direction. Ignored when
        /// `enabled` is false.
        speed: f32,
    },
    /// Keepalive. The client must answer with `ClientMessage::Pong`;
    /// silence past the configured timeout is a disconnect.
    Ping {
        nonce: u64,
    },
    /// The player's whole inventory.
    ///
    /// Sent as a snapshot rather than as deltas, and only when something
    /// changes. Forty slots of `Option<(u16, u32)>` is under half a
    /// kilobyte -- far less than one chunk -- and a snapshot cannot
    /// drift out of step with the server the way a stream of deltas can
    /// after a single dropped message.
    InventoryState {
        inventory: crate::inventory::Inventory,
    },
    /// What is in the chest the player has open.
    ///
    /// A snapshot, for the same reason the inventory is one: forty slots
    /// is under half a kilobyte, and a snapshot cannot drift out of step
    /// with the server the way a stream of deltas can after one dropped
    /// message. Sent when the chest is opened and after every change to
    /// it -- including changes another player made, so two people at one
    /// chest see the same thing.
    ChestState {
        global_x: i32,
        global_y: i32,
        global_z: i32,
        inventory: crate::inventory::Inventory,
        /// Which screen to draw. See `ContainerKind`.
        kind: ContainerKind,
        /// The fire, for a hearth. `None` for a chest, which has none --
        /// an `Option` rather than zeroes, because "no fire here" and "a
        /// fire that has gone out" are different things and the screen
        /// draws them differently.
        hearth: Option<HearthState>,
        /// The weather, for a drying rack, on exactly the same terms.
        rack: Option<RackState>,
    },
    /// The open chest is gone -- broken, or out of range. The client
    /// shuts the screen; anything else would leave it showing a chest
    /// that no longer exists.
    ChestClosed,
    /// Current and maximum health.
    ///
    /// Sent only when the value actually changes, not every tick: health
    /// is static for most of a session, and a per-tick broadcast would
    /// cost more bandwidth than player movement does.
    /// How much air is left, 0..1, and only while it is running out.
    ///
    /// Its own message rather than a field on `Health`, because the two
    /// change on completely different schedules: health changes when
    /// something happens, and breath changes every tick of a dive and
    /// never again. Sent only when it is *not* full, so a player who
    /// never puts their head under water never receives one.
    Breath {
        fraction: f32,
    },
    Health {
        current: f32,
        max: f32,
    },
    /// Blood where somebody can see it: the burst of a blow that landed on a
    /// person, or a drop off an open cut.
    ///
    /// **Sent to everybody near, the player it came off and the player who
    /// struck included, and predicted by nobody.** The attacker sees the blood
    /// a tick after the click rather than on it, and that is the price of the
    /// arrangement block edits (`SetBlock` is never predicted) and animals
    /// (their hurt flash rides the entity snapshot) already have: one source,
    /// so a blow is never drawn twice and never drawn at all when the server
    /// refused it. Prediction with an echo to everyone else was rejected -- a
    /// swing out of reach or inside the cooldown would bleed on the attacker's
    /// screen and nowhere else, and the server would have to know which of its
    /// messages answered which prediction.
    ///
    /// **Only for a blow or a cut** (`injury::Blow::drops`,
    /// `Injuries::drips_per_second`). It replaces the client drawing a burst
    /// for every point of health that went, which drew an illness, hunger or
    /// the cold as a blow twenty times a second: a raw fish was a fountain.
    Blood {
        /// Where, in world coordinates.
        at: (f64, f64, f64),
        /// How many drops.
        drops: u8,
    },
    /// How full the player is, 0..1.
    ///
    /// Its own message rather than a field on `Health`, and for the
    /// opposite reason to `Breath`'s: breath changes fast and rarely,
    /// health changes rarely and suddenly, and nourishment changes
    /// *slowly and always*. Folding it into `Health` would turn a
    /// message sent when something happens into one sent every few
    /// seconds forever.
    ///
    /// Sent on change past a threshold the server keeps, which for a
    /// bar drawn twenty segments wide is about a twentieth.
    Nourishment {
        fraction: f32,
    },
    /// What the sky is doing, for everyone.
    ///
    /// Sent on join and whenever it changes -- which is a few times an
    /// hour, so this is the cheapest message in the protocol. Rain is
    /// the world's, not the client's: two players in one field have to
    /// be standing in the same weather, and the moment it puts a fire
    /// out it stops being decoration. See `primitive_shared::weather`.
    WeatherSync {
        weather: crate::weather::Weather,
    },
    /// How warm the player is and how much water they have left.
    ///
    /// **One message for two meters**, unlike breath and hunger, and the
    /// reason is that they are coupled: heat is most of what drives
    /// thirst (`body::thirst_multiplier`), so the two numbers change
    /// together and a client that had one without the other would draw a
    /// gauge that disagrees with the reason it is moving.
    ///
    /// Sent on change past a threshold the server keeps, on exactly the
    /// terms `Nourishment` is -- these move slowly and continuously, and
    /// a per-tick message for a bar that takes ten minutes to cross
    /// would be more traffic than the movement it accompanies.
    ///
    /// `comfort` is derived rather than sent for its own sake: the
    /// thresholds live in `body::Comfort::of`, and having the client
    /// apply them itself would be two copies of five constants that must
    /// agree.
    Body {
        /// Skin temperature, in the degrees `body` measures in.
        temperature_c: f32,
        comfort: crate::body::Comfort,
        /// Water left, 0..1.
        hydration: f32,
        /// How tired, 0 (fresh) .. 1 (finished).
        ///
        /// **On this message rather than its own**, because it is a
        /// gauge that moves slowly and the gate that decides when to
        /// send this one (`Vitals::needs_body_report`) is exactly the
        /// gate it wants: a number that crawls should be sent when it
        /// has crawled far enough, not twenty times a second.
        fatigue: f32,
        /// How fast stamina comes back, as a multiplier: what the player's
        /// hidden comfort is worth (`comfort::recovery`).
        ///
        /// **The multiplier and not the comfort**, because stamina is the
        /// client's to predict and this is all the prediction needs; the
        /// value itself, and everything it is added up from, stays on the
        /// server where nobody draws it. Sent on this message's gate, a
        /// twentieth of a change at a time -- it settles over tens of
        /// seconds, and a bar that fills a hair faster a moment late is
        /// nothing anybody can see.
        recovery: f32,
        /// How wet, 0 (dry) .. 1 (soaked).
        ///
        /// **Sent because the pack screen now has a page that reads out
        /// every vital**, and wetness is the one that explains the other
        /// three: a soaked player is cold because they are soaked, their
        /// clothes have stopped insulating, and their comfort is on the
        /// floor. It was on the server only, which meant the health page
        /// could show the consequence and not the cause.
        ///
        /// On this message rather than its own, for `fatigue`'s reason:
        /// it crawls, and this message's gate is the gate a crawling
        /// number wants.
        wetness: f32,
        /// How filthy, 0 (clean) .. 1 (caked). See `comfort::step_grime`.
        grime: f32,
        /// How many food groups are in the player's recent diet, which is
        /// what decides how fast a wound closes
        /// (`food::diet_regen_factor`).
        ///
        /// A count and not the groups themselves: the number is the whole
        /// of what the rule reads, and sending which four would put a
        /// list on the wire so the screen could say something the rule
        /// does not care about.
        diet_groups: u8,
    },
    /// Every wound on the player's body, whole.
    ///
    /// **A snapshot, like the pack and the worn set**, and for their
    /// reason: six parts of four wounds is a little over a hundred bytes,
    /// and a snapshot cannot drift out of step after one dropped message the
    /// way a stream of "the left arm is bandaged now" would.
    ///
    /// Sent on join, and after that when something worth drawing changed
    /// -- a wound opened, closed or was dressed, or mended by a shade of the
    /// mannequin's red (`injury::Injuries::worth_reporting`). Its own
    /// message rather than a field on `Body`, because the two change on
    /// different clocks: the gauges crawl every tick, and a body is cut
    /// when something happens.
    ///
    /// It used to be one flag on `Body` -- "a leg is broken" -- and that
    /// note said a countdown on screen would turn an injury into a progress
    /// bar. The severities here are not a countdown: the client draws them
    /// as how bad a part looks, which is what a player can see of their own
    /// arm, and never as seconds left.
    Injuries {
        injuries: crate::injury::Injuries,
    },
    /// The player fell asleep, or woke up.
    ///
    /// **The client stops predicting while this is true.** A sleeping
    /// player is not moved by their own keys -- the server ignores
    /// their transforms -- so a client that went on simulating would
    /// walk away from the bed and be dragged back every tick. It also
    /// draws the screen that says what is happening, because a game
    /// that quietly ignores the controls is a game that has frozen.
    Asleep {
        asleep: bool,
    },
    /// The player sat down, lay down or got up, and where the server put
    /// them to do it.
    ///
    /// **A message and not a correction**, because it is not one: nothing
    /// the client did was wrong. The body is moved onto the seat or the
    /// mattress so that everyone else sees it there, and moved off it again
    /// to a free cell beside the piece -- see `standing_place` -- so a
    /// sleeper never wakes with a headboard through their shoulders. `at`
    /// is `None` when the posture ends where the player already is (they
    /// walked off a stool). `yaw` is the way the bed runs, from the middle
    /// toward the head, and means nothing for the other two.
    Posture {
        posture: Posture,
        at: Option<(f64, f64, f64)>,
        yaw: f32,
    },
    /// The player took the oars of this raft, or let go of them (`None`).
    ///
    /// **What turns the client's movement keys into strokes.** Until this
    /// arrives the keys walk the body about the deck; after it they row, and
    /// the client starts predicting the raft (`ClientMessage::Row`). Sent
    /// beside a `Posture` of sitting or standing, which is what moves the
    /// body onto the rower's seat and what everyone else sees.
    Oars {
        raft: Option<EntityId>,
    },
    /// What the player has on.
    ///
    /// A snapshot, like `InventoryState` and for the same reason: four
    /// slots is a hundred bytes, and a snapshot cannot drift out of step
    /// with the server after one dropped message the way a stream of
    /// deltas can.
    EquipmentState {
        equipment: crate::inventory::Equipment,
    },
    /// The player's health reached zero. The client shows this and asks
    /// for `ClientMessage::Respawn` when the player is ready.
    ///
    /// Carries its own text because the server is the only side that
    /// knows *why* -- the client cannot tell a fall from a drowning, and
    /// "you died" with no cause is the kind of thing players reload a
    /// save over.
    Died {
        cause: String,
    },
    /// Health restored and the player put back at the spawn point.
    Respawned {
        x: f64,
        y: f64,
        z: f64,
    },
    Kick(DisconnectReason),
    /// The answer to `ClientMessage::RequestExtensions`.
    ///
    /// Sent only when asked. An unprompted copy on join would be a
    /// message every player pays for so that the few who open the screen
    /// do not wait a round trip for it.
    Extensions(ExtensionList),
    /// Every kind of block this player has ever held, in id order.
    ///
    /// **The whole list, not what was just learned.** It is sent on join
    /// and then only when it grows, which is a handful of times an
    /// evening, and it is a few hundred bytes at the very end of the game
    /// -- while a delta that went missing would be a recipe the client's
    /// book never shows and nothing ever corrects. The same argument
    /// `InventoryState` makes. See `discovery` for what it decides.
    Discovered {
        kinds: Vec<BlockId>,
    },
    /// Where this player can find their way back to: the world's spawn,
    /// and every bag they died away from and have not yet emptied.
    ///
    /// **Sent to that player alone.** Where somebody's belongings are
    /// lying is theirs to share; a server that told everybody would turn
    /// every death into a race. Resent whenever the list changes -- a
    /// death adds one, emptying or losing a bag takes one away -- and on
    /// join, so the map is right before the player has opened it.
    Landmarks {
        spawn: (i32, i32, i32),
        bags: Vec<(i32, i32, i32)>,
    },
    Error(String),
    /// What a dead player's body at this cell is wearing, in
    /// `equipment::Slot` order, `BLOCK_AIR` for a bare slot.
    ///
    /// **A body wears what is in it**, read off the container rather than
    /// remembered from the moment of death: the first garment for each slot
    /// among its contents (`primitive_server::body_worn`). So a body that
    /// has been stripped of its cuirass is drawn without one, which is the
    /// thing a player walking back to their own grave most wants to see
    /// from a distance -- and nothing new is saved, because the contents
    /// already are.
    ///
    /// **Its own message, not bits of the block.** A corpse's variant field
    /// is three bits and a worn set is four block ids. Sent to everyone
    /// subscribed to the chunk, after the chunk itself and whenever the
    /// contents of a body change; a client that has not heard one draws
    /// the body bare. Rejected: carrying it in `ChestState`, which only the
    /// player looking into the body is sent.
    BodyWorn {
        x: i32,
        y: i32,
        z: i32,
        worn: [BlockId; crate::equipment::SLOTS],
    },
    /// What pottery is in the pit kiln at this cell, in the order it went
    /// in: raw or fired vessels, moulds, jugs and bricks.
    ///
    /// **The report: "you can't see what the player places -- a brick or a
    /// mould looks like jugs".** The pit's id says how many pieces and
    /// whether they are fired (`pit::Stage`), because that fits the variant
    /// field and a mixture of four kinds does not; so the mesher drew *that
    /// many pots*. This says which, and the mesher draws each as itself.
    ///
    /// `BodyWorn`'s arrangement exactly, for its reason: sent right behind
    /// the chunk the pit is in and to everyone subscribed whenever what is
    /// in it changes, and a client that has not heard one draws plain pots,
    /// which is what every build before this drew. An empty list is a pit
    /// with nothing in it any more.
    PitPottery {
        x: i32,
        y: i32,
        z: i32,
        pieces: Vec<BlockId>,
    },
    /// How thick the smoke is where this player's head is, 0..1: the smoke
    /// of a fire in a closed room (`wildfire::smoke_room`).
    ///
    /// Sent when it changes by a step worth drawing, and once more at
    /// nought, so the fog of a smoky room lifts when the door is opened --
    /// `Breath`'s rule, and `Breath`'s bug if it were not.
    Smoke {
        thickness: f32,
    },
    /// What lies in the cell a hand set something down in
    /// (`types::BLOCK_SET_DOWN`): the thing, or air for nothing.
    ///
    /// `PitPottery`'s arrangement exactly, for its reason: the id says a
    /// thing lies there and which way, and six hundred kinds do not fit in
    /// its variant field. Sent right behind the chunk and to everyone
    /// subscribed whenever what is in the cell changes; a client that has
    /// not heard draws nothing there, which is a gap and never a wrong
    /// object. Only the kind -- a knife is drawn as a knife however worn.
    SetDownItem {
        x: i32,
        y: i32,
        z: i32,
        item: BlockId,
    },
    /// Somebody who was already here when this client came: their name.
    ///
    /// **A newcomer never learnt who was already in the world.** Names travel
    /// in `PlayerJoined`, which goes to everybody *else* at the moment a
    /// player arrives, so the one who arrived last knew nobody's name -- and
    /// when one of them left, the chat said nothing at all, because a leaver
    /// with no name is a leaver the client cannot announce. Its own message
    /// rather than a `PlayerJoined` sent late, because the client says "joined"
    /// in the chat for that one, and a newcomer greeted by a list of people
    /// joining who had been there an hour would be told something untrue.
    /// Appended, so it rides fifty-seven's bump.
    PlayerPresent {
        id: PlayerId,
        username: String,
    },
    /// The station screen may open: which game it is, and how wide the sweet
    /// spot will be.
    ///
    /// **The width comes from the server**, though the client could work it
    /// out from the hammer in its own hand. It is the number the verdict is
    /// judged by (`minigame::tolerance`), and a client drawing one width while
    /// the server scores by another is a screen that lies about a near miss.
    /// One value, decided once, carried -- the same argument `Heat` carries
    /// for the crafting menu.
    StationOpen {
        game: crate::minigame::Game,
        tolerance: f32,
    },
    /// The job's materials are spent and the run has begun. The seed decides
    /// where the sweet spots are, so the client cannot know them before this
    /// arrives and cannot choose them at all.
    StationBegun {
        seed: u32,
    },
    /// How the run went. The pack follows in the usual `Inventory` message;
    /// this is what the screen shows the player.
    StationResult {
        verdict: crate::minigame::Verdict,
        made: Option<(BlockId, u32)>,
    },
    /// **What the line is doing**, to the one player holding it.
    ///
    /// Sent when the phase changes, and once a tick while a fish is on
    /// (the strain moves every frame and is what the player is playing
    /// against). `None` for the float is the line out of the water, however
    /// it came out.
    ///
    /// **One message rather than four.** "There is a float here", "it
    /// dipped", "the fish is pulling this hard", "it is gone" are one
    /// picture that the client draws in one place, and four messages would
    /// be four chances for them to arrive in the wrong order and leave a
    /// float bobbing over a line that is not there.
    ///
    /// **`liveliness` is the spot, not the fish**: 0 for water where a bite
    /// is minutes away, 255 for water where one is seconds away. The client
    /// twitches the float that often, which is how a player reads a spot
    /// without a number on the screen (`fishing` and `logic::fishing`).
    Line {
        float: Option<(i32, i32, i32)>,
        /// `fishing::Phase`, as a byte: see `logic::fishing::Phase::code`.
        phase: u8,
        /// How near the line is to parting, 0 to 255.
        strain: u8,
        /// How well this water is fishing, 0 to 255.
        liveliness: u8,
    },
    /// **Somebody has this chest open**, or nobody has any more.
    ///
    /// Sent to everyone subscribed to the chunk and not only to whoever
    /// opened it: a chest is a thing two players stand at, and a lid that
    /// moved for one of them was the reason this is a message at all.
    ///
    /// **A message and not a bit on the block.** The block was the first
    /// answer -- `types::DOOR_OPEN` is exactly that, and the swing of a door
    /// is broadcast as a `BlockUpdate` -- and it has two faults a lid has
    /// and a door does not. It is *saved*: a world whose process was killed
    /// with somebody's hand in a chest loads with that chest open for ever,
    /// and no rule would ever shut it, because the rule that would is "is
    /// anybody standing at it" and nobody is. And a lid takes a third of a
    /// second to move (`mesh::LID_SWING_SECONDS`), so the client has to keep
    /// the lid out of the chunk mesh until it has finished falling, which is
    /// a state the block id cannot hold without the mesh disagreeing with it
    /// for that third of a second -- the two lids, one swinging and one lying
    /// shut through it.
    ///
    /// **A player who arrives after the chest was opened is told with the
    /// chunk.** It used to be the thing this gave up -- nobody said, and they
    /// saw it shut until it was closed -- and so did a player whose chunk was
    /// unloaded and loaded again while a lid was up, because the client
    /// forgets a chunk's lids with the chunk. The server now sends one of
    /// these, `open: true`, behind every chunk for every chest in it that
    /// somebody is at (`primitive_server::open_lids_in`): the one message on
    /// the stream this note once priced, and cheaper than a chest standing
    /// open for ever with nobody at it.
    ///
    /// Appended, so it rides fifty-seven's bump.
    ChestLid {
        x: i32,
        y: i32,
        z: i32,
        open: bool,
    },
    /// What came out of the water on the end of the line: the fish, by kind.
    ///
    /// **The species, not a sentence**, for the reason the lost line is not
    /// a sentence either (`Msg::FishingLineGone`): the words belong to the
    /// player's language, and the server does not know which that is. The
    /// client names it (`ui::names::animal`) -- which is the first place a
    /// player was ever told what they caught; before this a trout and a pike
    /// were both "raw fish" arriving in the pack.
    ///
    /// Appended, so it rides fifty-seven's bump.
    Caught {
        species: crate::animals::Species,
    },
    /// A body, a player's or an animal's, driven onto sharpened stakes
    /// (`spikes`) at `at` -- told to everybody near, the victim included,
    /// so each of them hears it where it happened.
    ///
    /// **Its own message because the cause never reaches the client
    /// otherwise.** `Health` says how much is left and not what took it,
    /// and `Blood` is any blow or cut; a sound keyed off either would play
    /// the stakes for a boar's tusk. Putting a cause on `Health` was the
    /// other way and was rejected: it is sent to one player, and a stake
    /// going into somebody -- or into a deer at the palisade -- is heard by
    /// everybody standing near. Appended last so every earlier message
    /// keeps its tag.
    Staked {
        at: (f64, f64, f64),
    },
    /// **A rack said no because the thing is the other rack's work**: a skin
    /// offered to the rack of two by two, a fish to the hide frame
    /// (`rack::refused_for_its_trade`). `rack` is the trade of the one that
    /// refused.
    ///
    /// **The trade, not a sentence**, for `Caught`'s reason: the words belong
    /// to the player's language, and `Error` carries English. Sent only for
    /// this refusal -- a stone dragged onto a rack is refused in silence, as
    /// it always was, because nobody needs telling a stone does not dry. The
    /// words matter here because the rack a player has used for a month
    /// stopped taking hides the day it became the larder, and a refusal with
    /// no reason reads as a broken rack. Appended, so it rides fifty-seven's
    /// bump.
    RackRefused {
        rack: crate::rack::Trade,
    },
    /// **What the place a player stands in is doing to their warmth**: the
    /// air, whether it is a room, how draughty, what the walls are worth,
    /// and whether a smoke hole is letting the fire's heat out
    /// (`shelter::Reading`).
    ///
    /// For the health page, which showed the skin's temperature and not the
    /// reason for it -- and a room that is cold because its door faces the
    /// wind is a room the player can fix only once they are told. Its own
    /// message and not more fields on `Body`, because it changes on a
    /// different clock: `Body` crawls with the skin, and this jumps when a
    /// door opens. Sent on `Reading::differs`. Appended, so it rides
    /// fifty-seven's bump.
    Shelter {
        reading: crate::shelter::Reading,
    },
}

/// Trims/sanitises a username before it's shown to other players or
/// written to a log. Runs on the *server* -- a client can send anything.
pub fn sanitize_username(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_USERNAME_LEN)
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        "player".to_string()
    } else {
        cleaned
    }
}

pub fn sanitize_chat(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(MAX_CHAT_LEN)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {

    /// **A tall chunk costs little on the wire, and comes back whole.**
    ///
    /// The world went from sixty-four blocks tall to two hundred and
    /// fifty-six, and sent raw a chunk went from 32 KB to 131 KB: four
    /// times the bandwidth for every chunk streamed to every player, for
    /// what is mostly more air and more stone. Run-length encoding is the
    /// answer and this is the check on both halves of it -- that a real
    /// generated chunk is small, and that nothing is lost getting there.
    #[test]
    fn a_tall_chunk_is_small_on_the_wire_and_arrives_intact() {
        let gen = crate::worldgen::WorldGen::new(1337);
        for cx in 0..6 {
            let chunk = gen.generate_chunk(crate::types::ChunkPos::new(cx, 2));
            let bytes = bincode::serialize(&chunk).unwrap();
            assert!(
                bytes.len() < 24 * 1024,
                "chunk {cx} is {} KB on the wire; raw it would be {} KB",
                bytes.len() / 1024,
                crate::types::CHUNK_VOLUME * 2 / 1024
            );
            let back: crate::types::Chunk = bincode::deserialize(&bytes).unwrap();
            assert_eq!(back.blocks, chunk.blocks, "chunk {cx} changed in transit");
            assert!(back.is_well_formed());
        }
    }

    /// A length prefix is a claim, and a claim about four billion cells
    /// is refused before any memory is spent on it.
    #[test]
    fn a_chunk_claiming_more_cells_than_a_chunk_holds_is_refused() {
        let runs: Vec<(u32, crate::types::BlockId)> = vec![(u32::MAX, 1), (u32::MAX, 2)];
        let bytes = bincode::serialize(&(crate::types::ChunkPos::new(0, 0), runs)).unwrap();
        assert!(bincode::deserialize::<crate::types::Chunk>(&bytes).is_err());
    }
    use super::*;

    #[test]
    fn username_sanitising_is_defensive() {
        assert_eq!(sanitize_username("  Shamkhan \n"), "Shamkhan");
        assert_eq!(sanitize_username(""), "player");
        assert_eq!(sanitize_username("\u{0}\u{1}"), "player");
        assert_eq!(sanitize_username(&"x".repeat(200)).len(), MAX_USERNAME_LEN);
    }

    #[test]
    fn chat_is_length_capped() {
        assert!(sanitize_chat(&"y".repeat(1000)).len() <= MAX_CHAT_LEN);
    }

    #[test]
    fn messages_roundtrip_through_bincode() {
        let msg = ServerMessage::Snapshot {
            tick: 7,
            states: vec![PlayerState {
                id: 1,
                x: 1.0,
                y: 2.0,
                z: 3.0,
                yaw: 0.5,
                pitch: -0.2,
                on_ground: true,
                outfit: Outfit::BARE,
                posture: Posture::Standing,
                limp: 0,
                limp_left: false,
                gesture: Gesture::default(),
            }],
        };
        let bytes = bincode::serialize(&msg).unwrap();
        let back: ServerMessage = bincode::deserialize(&bytes).unwrap();
        match back {
            ServerMessage::Snapshot { tick, states } => {
                assert_eq!(tick, 7);
                assert_eq!(states.len(), 1);
                assert_eq!(states[0].id, 1);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn a_dressed_player_arrives_dressed_and_still_holding_it() {
        use crate::equipment::Slot;
        use crate::types::{
            BLOCK_IRON_HELM, BLOCK_LEATHER_BOOTS, BLOCK_STONE_PICKAXE, BLOCK_WOOL_TUNIC,
        };
        // Three materials at once, on purpose: the four slots are one
        // fixed-size array on the wire, so a slot written at the wrong
        // offset would be a player in boots on their head -- and a set
        // of one material could not tell that apart from a set arriving
        // correctly.
        let mut outfit = Outfit::BARE;
        outfit.worn[Slot::Head.index()] = BLOCK_IRON_HELM;
        outfit.worn[Slot::Chest.index()] = BLOCK_WOOL_TUNIC;
        outfit.worn[Slot::Feet.index()] = BLOCK_LEATHER_BOOTS;
        outfit.holding = BLOCK_STONE_PICKAXE;

        let message = ServerMessage::Snapshot {
            tick: 3,
            states: vec![PlayerState {
                id: 9,
                x: -12.5,
                y: 64.0,
                z: 8.25,
                yaw: 1.5,
                pitch: 0.25,
                on_ground: false,
                outfit,
                posture: Posture::Lying,
                limp: 0,
                limp_left: false,
                gesture: Gesture::default(),
            }],
        };
        let bytes = bincode::serialize(&message).unwrap();
        let ServerMessage::Snapshot { states, .. } =
            bincode::deserialize::<ServerMessage>(&bytes).unwrap()
        else {
            panic!("wrong variant");
        };
        let back = states[0].outfit;
        assert_eq!(back.worn_in(Slot::Head), BLOCK_IRON_HELM);
        assert_eq!(back.worn_in(Slot::Chest), BLOCK_WOOL_TUNIC);
        assert_eq!(back.worn_in(Slot::Legs), crate::types::BLOCK_AIR);
        assert_eq!(back.worn_in(Slot::Feet), BLOCK_LEATHER_BOOTS);
        assert_eq!(back.holding, BLOCK_STONE_PICKAXE);
        assert!(!back.is_bare());
        // ...and the fields after it are still themselves, which is the
        // half a positional encoding gets wrong quietly.
        assert_eq!(states[0].id, 9);
        assert_eq!(states[0].yaw, 1.5);
        assert!(!states[0].on_ground);
    }

    #[test]
    fn dressing_a_player_costs_twelve_bytes_of_snapshot() {
        // The number the design argument in `Outfit` rests on. Twelve
        // bytes a tick is 240 B/s per visible player per recipient; a
        // length prefix on the worn set, or an `Option` tag per slot,
        // would be most of that again for nothing. If this ever changes,
        // the paragraph that justifies putting the outfit in the
        // snapshot at all has to change with it.
        //
        // **It was ten, for four garments and a held block.** The fifth
        // slot is the back (`equipment::Slot::Back`), which carries a
        // rucksack -- and a rucksack has to be in the snapshot for the
        // same reason a cuirass does: it is a thing other players see on
        // you, and a body on the ground is drawn wearing what it was
        // wearing. Two more bytes is the whole price.
        let bare = PlayerState {
            id: 1,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            outfit: Outfit::BARE,
            posture: Posture::Standing,
            limp: 0,
            limp_left: false,
            gesture: Gesture::default(),
        };
        let without = bincode::serialized_size(&(
            bare.id,
            bare.x,
            bare.y,
            bare.z,
            bare.yaw,
            bare.pitch,
            bare.on_ground,
            bare.posture,
            // `limp` is in the baseline, not in the outfit: it is a byte
            // of body language that arrived beside this one, and leaving
            // it out would charge the outfit for it.
            bare.limp,
            bare.limp_left,
            bare.gesture,
        ))
        .unwrap();
        let with = bincode::serialized_size(&bare).unwrap();
        assert_eq!(with - without, 12, "the outfit is {} bytes", with - without);
    }

    #[test]
    fn a_players_gesture_rides_the_snapshot_in_three_bytes() {
        // The argument in `Gesture`: a flag for what lasts, the last thing
        // that happens once, and a count of those -- three bytes, per player,
        // per tick. A count that wrapped is still a change.
        let mut gesture = Gesture { digging: true, ..Gesture::default() };
        for _ in 0..300 {
            gesture.made(Action::Strike);
        }
        gesture.made(Action::Drink);
        let bytes = bincode::serialize(&gesture).unwrap();
        assert_eq!(bytes.len(), 3, "a gesture took {} bytes", bytes.len());
        // A byte from a newer build is nothing rather than a failed snapshot.
        assert_eq!(bincode::deserialize::<Action>(&[200]).unwrap(), Action::Nothing);

        let message = ServerMessage::Snapshot {
            tick: 11,
            states: vec![PlayerState {
                id: 4,
                x: 1.0,
                y: 2.0,
                z: 3.0,
                yaw: 0.25,
                pitch: 0.0,
                on_ground: true,
                outfit: Outfit::BARE,
                posture: Posture::Sitting,
                limp: 0,
                limp_left: false,
                gesture,
            }],
        };
        let ServerMessage::Snapshot { states, .. } =
            bincode::deserialize::<ServerMessage>(&bincode::serialize(&message).unwrap()).unwrap()
        else {
            panic!("wrong variant");
        };
        assert_eq!(states[0].gesture, gesture, "the gesture changed in transit");
        assert!(states[0].gesture.digging && states[0].gesture.last == Action::Drink);
        assert_eq!(states[0].posture, Posture::Sitting, "the field before it was disturbed");
    }

    #[test]
    fn a_posture_costs_one_byte_and_survives_the_wire() {
        // The argument in `Posture` for serialising it through a `u8`: a
        // plain enum is a four-byte tag in bincode, per player, per tick.
        for posture in [Posture::Standing, Posture::Sitting, Posture::Lying] {
            let bytes = bincode::serialize(&posture).unwrap();
            assert_eq!(bytes.len(), 1, "{posture:?} took {} bytes", bytes.len());
            assert_eq!(bincode::deserialize::<Posture>(&bytes).unwrap(), posture);
        }
        // A byte from a newer build reads as standing rather than failing
        // the whole snapshot.
        assert_eq!(bincode::deserialize::<Posture>(&[9]).unwrap(), Posture::Standing);
    }

    #[test]
    fn the_wire_carries_a_filled_jug_unchanged() {
        use crate::inventory::{filled_jug, jug_contents, Inventory};
        use crate::types::BLOCK_GRAIN;
        // A jug's contents ride in `Stack::damage` (see
        // `inventory::jug_contents`), which is a field the wire already
        // had -- so this needed no new bytes and no new shape. What it
        // *does* need is for the receiving end's `sanitize` to leave it
        // alone, and that is the half this catches: the client calls it
        // on every snapshot, and it used to clear the wear on anything
        // that cannot wear out.
        let mut inventory = Inventory::new();
        inventory.put_in_slot(2, filled_jug(BLOCK_GRAIN, 11));
        let message = ClientMessage::PourIntoJug { from: 0, jug: 2 };
        let bytes = bincode::serialize(&message).unwrap();
        assert!(matches!(
            bincode::deserialize::<ClientMessage>(&bytes).unwrap(),
            ClientMessage::PourIntoJug { from: 0, jug: 2 }
        ));

        let bytes = bincode::serialize(&ServerMessage::InventoryState {
            inventory: inventory.clone(),
        })
        .unwrap();
        let ServerMessage::InventoryState { mut inventory } =
            bincode::deserialize::<ServerMessage>(&bytes).unwrap()
        else {
            panic!("wrong variant");
        };
        inventory.sanitize();
        assert_eq!(
            jug_contents(&inventory.slots()[2].unwrap()),
            Some((BLOCK_GRAIN, 11))
        );
    }
}

#[cfg(test)]
mod entity_tests {
    use super::*;
    use crate::types::BLOCK_SAND;

    #[test]
    fn a_body_on_the_stakes_reaches_the_client_where_it_happened() {
        let message = ServerMessage::Staked { at: (12.5, 64.5, -3.25) };
        let bytes = bincode::serialize(&message).unwrap();
        match bincode::deserialize(&bytes).unwrap() {
            ServerMessage::Staked { at } => assert_eq!(at, (12.5, 64.5, -3.25)),
            other => panic!("wrong message: {other:?}"),
        }
        // Appended last, so no message that shipped before it changed tag.
        let caught = bincode::serialize(&ServerMessage::Caught { species: crate::animals::Species::Deer }).unwrap();
        let tag = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        assert_eq!(tag(&bytes), tag(&caught) + 1, "Staked is not the message after Caught");
    }

    #[test]
    fn entity_snapshots_survive_a_round_trip() {
        let states = vec![EntityState {
            id: 7,
            kind: EntityKind::FallingBlock { block: BLOCK_SAND },
            x: 1.5,
            y: 40.25,
            z: -3.5,
        }];
        let message = ServerMessage::Entities { tick: 99, states };
        let bytes = bincode::serialize(&message).unwrap();
        let decoded: ServerMessage = bincode::deserialize(&bytes).unwrap();

        match decoded {
            ServerMessage::Entities { tick, states } => {
                assert_eq!(tick, 99);
                assert_eq!(states.len(), 1);
                assert_eq!(states[0].id, 7);
                assert_eq!(states[0].kind, EntityKind::FallingBlock { block: BLOCK_SAND });
                assert_eq!(states[0].y, 40.25);
            }
            other => panic!("wrong message: {other:?}"),
        }
    }

    #[test]
    fn two_simulations_counting_from_one_still_never_issue_the_same_id() {
        // The whole point of the source bits. Both counters run over
        // the same range, which is what they did in the world where a
        // falling block and an animal shared a row in the client's
        // entity table.
        for ordinal in 1..1000u64 {
            let block = entity_id(EntitySource::FallingBlock, ordinal);
            let item = entity_id(EntitySource::Item, ordinal);
            let animal = entity_id(EntitySource::Animal, ordinal);
            assert_ne!(block, item);
            assert_ne!(block, animal);
            assert_ne!(item, animal);
        }
    }

    #[test]
    fn an_entity_id_says_which_simulation_made_it() {
        assert_eq!(
            entity_source(entity_id(EntitySource::FallingBlock, 42)),
            Some(EntitySource::FallingBlock)
        );
        assert_eq!(
            entity_source(entity_id(EntitySource::Animal, 42)),
            Some(EntitySource::Animal)
        );
        // A bare count is nobody's -- which is what every id in a save
        // or a log from before the sources existed looks like.
        assert_eq!(entity_source(42), None);
    }

    #[test]
    fn a_source_never_swallows_the_count_it_was_given() {
        // The ordinal has to survive the packing, or two entities from
        // one simulation would collide instead -- the same bug moved
        // rather than fixed.
        let mut seen = std::collections::HashSet::new();
        for ordinal in 1..10_000u64 {
            assert!(
                seen.insert(entity_id(EntitySource::Item, ordinal)),
                "ordinal {ordinal} reused an id"
            );
        }
    }
}
