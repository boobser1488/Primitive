//! Why each shipped model is shaped the way it is.
//!
//! The models are files now (`assets/models`, read by `logic::models`),
//! and a `.bbmodel` has nowhere to say *why* an ear is a fifth of a
//! sixteenth lower than the head. These are the comments the tables in
//! `animal_model` and `mesh` carried when the files were written out of
//! them, kept word for word under the file and the box they were about --
//! so the next person moving a box in Blockbench can read what it was
//! moved there to fix before moving it back.
//!
//! Where a note names a table (`BOAR`, `GULL`), it means
//! that model's file; where it says "this file" or "this table", the same.
//! A box's note is a `- **name**:` line, and
//! `every_box_a_note_is_about_is_in_its_file` keeps them honest:
//! rename a box in Blockbench and rename its note here.
//!
//! ## animals/boar.bbmodel
//!
//! The boar.
//!
//! Nine boxes. Everything about the shape is here and nowhere else, so
//! making it heavier in the shoulder is changing one line.
//!
//! It is built to stand exactly one block at the shoulder, because that
//! is what `Species::Boar.height()` says and the collider is built from
//! the same number -- a model taller than its collider is an animal you
//! cannot hit in the head.
//!
//! - **head**: Carried low and forward, the way a boar carries it -- a head level
//!   with the back reads as a pig, and a head above it as a dog.
//!   **The base skin is the plain hide, not the eyed side view.** A head
//!   box has six faces and only two of them have an eye in them; the
//!   other four are the top of the skull, the back of it and the
//!   underside of the jaw. Wearing `Skin::Head` on all six -- which is
//!   what every head in this file used to do -- puts an eye on top of
//!   the animal and another on the back of its neck.
//!   Its underside a fifth of a sixteenth over the body's rather than in
//!   the same plane, where the two flickered (`model_overlap`).
//!
//! - **tusk left**: Up and out of the lower jaw. Small: two pixels of tusk is what
//!   says "boar" at ten metres, and four says "warthog".
//!   A fifth of a sixteenth forward of the snout's back, which it used to
//!   share: two faces in one plane flicker (see `model_overlap`).
//!
//! ## animals/deer.bbmodel
//!
//! The deer: the same skeleton, stretched.
//!
//! Taller, narrower, longer in the leg and the neck. All hide, because a
//! deer has no part that is not the colour of a deer -- and that is the
//! whole reason the `Skin` column exists rather than every animal
//! needing five pictures.
//!
//! - **neck**: A fifth of a sixteenth narrower than the head each side: as wide as
//!   it, the two sides were one plane in two pictures, and flickered.
//!
//! - **muzzle**: The muzzle: a narrower box in front of the head, bare skin all
//!   round and nostrils on the end. Without it the front of a deer is
//!   the *side* of its head, eye and all.
//!   Its underside a fifth of a sixteenth over the head's, which it used
//!   to share (`model_overlap`).
//!
//! - **ear left**: Lowered a pixel from where they were, to make room overhead for
//!   the antlers without the drawn animal outgrowing its collider --
//!   see `a_model_is_the_size_of_the_animal_the_server_is_simulating`.
//!
//! - **antler left beam**: **Antlers, in four boxes: a beam and a tine a side.**
//!
//!   The fork is geometry rather than texture, because a box one pixel
//!   wide has no room to draw a branch -- and the fork is the whole
//!   silhouette. Two boxes is the fewest that reads as one from any
//!   angle: a single spike a side is a pair of horns, which is a goat.
//!   Rooted *inside* the skull rather than balanced on top of it, which
//!   is both how an antler grows and what keeps the drawn deer inside
//!   the third of a block of slack the collider allows over its
//!   shoulder height. Four pixels of beam is as much as that slack
//!   pays for.
//!
//!   The beams stand a fifth of a sixteenth in from the skull's sides,
//!   which they used to be flush with (`model_overlap`).
//!
//! ## animals/wolf.bbmodel
//!
//! The wolf.
//!
//! Ten boxes, and the whole of what makes it read as a wolf rather than
//! as a small deer is where the head is: carried *level with the back*
//! and pushed forward on a neck that slopes rather than stands. A head
//! above the shoulder is a deer, a head below it is a boar, and a head
//! in line with it is every dog that ever lived.
//!
//! The other half of it is the tail, which is the one part here that is
//! a whole box rather than a flap: a wolf's tail is as thick as its
//! foreleg and it is what a player sees when the animal is walking away
//! from them, which -- for a lone one -- is most of the time.
//!
//! - **neck**: A fifth of a sixteenth narrower than the head each side. **Every
//!   change on this model marked `model_overlap` is that fifth**: two
//!   faces that point the same way from one plane are two pictures the
//!   depth buffer picks between by rounding, and the neck's sides against
//!   the head's, the ears' fronts against the neck's, the muzzle's
//!   underside against the head's and the legs' outsides against the
//!   body's all flickered as the animal moved.
//!
//! - **muzzle**: The long one. A wolf's muzzle is a third of the length of its head
//!   and it is the single line that separates the silhouette from a
//!   dog's -- or, at this scale, from a deer's.
//!
//! - **ear left**: Upright and pointed, which is the silhouette that says wolf at
//!   the distance one is usually first seen from.
//!
//! ## animals/hare.bbmodel
//!
//! The hare: small, and mostly ears.
//!
//! - **muzzle**: Short and blunt, which is what a hare has. Two pixels, so it is
//!   almost all nose.
//!
//! - **ear left**: The one part of a hare anybody could pick out of a field. They
//!   stand above the collider, and that is allowed: see
//!   `a_model_is_the_size_of_the_animal_the_server_is_simulating`.
//!
//!   A fifth of a sixteenth in from the skull's sides and its back, both of
//!   which they used to share a plane with (`model_overlap`); the haunches
//!   stand that much proud of the body and the tail that much under its
//!   back, for the same reason.
//!
//! - **haunch left**: Drawn back and low, which is the only thing that makes a box read
//!   as something that bolts.
//!
//! ## animals/fish.bbmodel
//!
//! **A fish: a body, a head with a mouth on it, a tail and three fins.**
//!
//! Built to the same tests every animal here answers to, and they turn out
//! to describe a fish well: a head with a face in front and an eye either
//! side, a muzzle -- the lips -- narrower than the skull and ahead of it,
//! and "legs" that swing in opposite pairs, which on a fish are the two
//! pectoral fins paddling as it goes. No ears, for the bird's reason.
//!
//! **The tail does not wag, and that is the thing this model is missing.**
//! A leg swings about its top in the plane of the animal's length; a tail
//! beats side to side, which is a turn about the vertical axis through the
//! root of the tail -- a second kind of swing in `build` and in the pose
//! every carcass and skeleton shares. Rejected for now rather than done
//! badly: the fins paddling already say "swimming" from any distance a
//! player sees a fish from through the water, and a yaw-swing added to
//! the posing code is a change to every animal's pose for one animal's
//! tail.
//!
//! `Species::Fish.height()` is its depth, fin to belly, and the collider
//! the server keeps inside the water is that box.
//!
//! - **tail**: Tall and thin, standing across the back of the body: the one
//!   silhouette that reads as a fish from the side through fog.
//!
//! ## animals/cod.bbmodel
//!
//! **A cod: the fish, heavier.** Deeper in the body for its length, a
//! broad head and a heavy jaw, and a tail as tall as the body. See `FISH`
//! for what the tests ask of a fish and what the model does not do.
//!
//! ## animals/zebra.bbmodel
//!
//! The zebra: a striped horse.
//!
//! **What makes it a horse rather than a big deer is the neck and what sits
//! on it**: a column of a neck instead of a stalk, a long head held out at
//! the top of it, a mane standing along the crest and a tail that hangs.
//! The stripes are the sheet's (`animals/zebra.png`); the mane wears `Fur`,
//! which that sheet draws as short upright stripes, so the crest reads as
//! hair standing up rather than as the hide going on.
//!
//! Built to `Species::Zebra.height()` at the shoulder with the ears inside
//! the third of a block of slack the collider allows over it -- which is
//! what holds the head lower than a horse at attention would carry it.
//!
//! - **mane**: A crest of hair along the back of the neck, a sixteenth proud of it
//!   behind and two over its top. Thin, because a mane a whole neck wide is
//!   a second neck.
//!
//! - **muzzle**: Long and dark: the black nose is the one part of a zebra that is not
//!   striped, and it is what makes the head read as a horse's at a glance.
//!
//! - **ear left**: A quarter of a sixteenth in from the skull's sides, not 0.15, which
//!   flickered against them at a distance (`model_overlap`).
//!
//! - **tail**: Hanging, not held out: a horse's tail is a rope, a wolf's is a brush.
//!
//! ## animals/antelope.bbmodel
//!
//! The antelope: small, slender, and known by two things -- the horns
//! swept back off its crown and the white belly it shows as it turns.
//!
//! **The horns are two boxes a side**, an upright root and a tip laid back
//! along the skull, for the reason the deer's antlers are a beam and a
//! tine: one straight spike a side is a goat. They wear `Skin::Horn`, not
//! the deer's `Antler`. The belly is a slab under the body in `Fur`, which
//! this sheet draws white, set in from the flanks so it shows below them
//! as a pale line rather than as a second body.
//!
//! - **ear left**: Out to the side more than up: an antelope's ears are wide and they are
//!   what turns toward you first.
//!
//! - **horn left**: **Every number on this model marked `model_overlap` moved by a fifth
//!   of a sixteenth or so, for one reason**: two faces pointing the same
//!   way from one plane are two pictures the depth buffer picks between by
//!   rounding. The legs' outsides were flush with the belly's, the horns'
//!   fronts 0.15 from the ears', and each tip as wide as its root; the
//!   tips are thinner now, which is also what the end of a horn is.
//!
//! ## animals/lion.bbmodel
//!
//! The lion: a long low body, a heavy head, and the mane.
//!
//! **The mane is the whole animal at a distance**, so it is one box and a
//! big one: wider and taller than the head, round the neck and the back of
//! the skull, in `Fur` (dark brown on this sheet). What it must not do is
//! swallow the face. The head stands out of it by most of its own length,
//! so the eye drawn toward the front of the head's side picture is in the
//! open -- see `a_lions_mane_is_round_its_head_and_behind_its_face`.
//!
//! The tail is the hide's colour and ends in a dark tuft, which is the other
//! thing that says lion rather than a big sandy dog.
//!
//! - **muzzle**: Broad and blunt: a cat's face is short, and a long muzzle is a dog's.
//!
//! - **ear left**: Small and round, in front of the mane rather than inside it, or they
//!   are a lump in the hair.
//!
//! - **tail tuft**: Its top a quarter of a sixteenth under the tail's, and the legs a
//!   quarter inside the flanks: a tenth, which they were, is inside what
//!   the depth buffer holds apart at a distance (`model_overlap`).
//!
//! ## animals/bear.bbmodel
//!
//! The bear.
//!
//! **The silhouette is the warning, and the first bear drawn did not give
//! it.** A player said it looked like a capybara, and measured, it did: one
//! box fifteen sixteenths deep on legs that showed five of their eight, a
//! head as tall as half the body, and a snout whose front face *was* the
//! head's front face -- a muzzle that stood out of the face by exactly
//! nothing. A hump half a sixteenth over the back is not a hump. A barrel
//! with a flat blunt face on stubby legs is a rodent. **Size was not the
//! fault**: that bear filled more room than this one (1.42 cubic blocks
//! against 1.25), nearly all of it in the barrel.
//!
//! What says bear, in the order the eye takes it in across a clearing:
//!
//! * **the hump is the top of the animal**, 2.4 sixteenths over the rump,
//!   so the back slopes down to the tail. `shoulders` and `hump` stand over
//!   the forelegs, `body` is lower behind them, `neck` and `head` lower
//!   still in front: a staircase of four steps that reads as a slope;
//! * **a heavy head carried low**, its top under the line of the rump, with
//!   a short broad muzzle that plainly stands out of the face and small
//!   round ears at the back of the skull;
//! * **long thick forelegs under a deep chest** that hangs below the belly,
//!   on **paws** broader than the leg and reaching forward of it -- a flat
//!   foot, which a hoof-coloured column was not;
//! * a stub of a tail -- the old one was a box five sixteenths long -- and
//!   the length it gave up put where it shows, in a neck and a muzzle
//!   (`Species::Bear.length`).
//!
//! **No two faces that point the same way share a plane or come within
//! 0.3 of a sixteenth of one another** where they overlap. `SEAM_BITE`
//! settles two boxes that only touch; it cannot settle two parallel faces a
//! tenth apart, which the depth buffer cannot tell apart at thirty blocks
//! (the arithmetic is at `CLEARANCE`). That is why the paws are 5.6 wide on
//! a 5-wide leg and the neck is wider than the head.
//!
//! It stands 1.3 blocks at the shoulder because `Species::Bear.height()`
//! says so, and the box the server validates blows against is built
//! from the same numbers: see `the_shared_hit_box_is_the_model_that_is_drawn`.
//! The shape is held by
//! `a_bear_is_humped_at_the_shoulder_with_its_head_low_on_long_forelegs`.
//!
//! The bear's joints, as `(y, z)` in sixteenths (see `Part::pivot`): where
//! the neck leaves the shoulders, which the head, muzzle and ears nod about
//! together, and the tops of the legs, which each paw swings about with
//! its leg. Named once so a leg and its paw cannot be given two.
//!
//! - **body**: The barrel, from behind the shoulders to the rump. Its top is the
//!   rump; the shoulders and the hump stand on the front of it.
//!
//! - **shoulders**: Over the forelegs, and bigger than the barrel every way: wider,
//!   higher, and deeper -- the chest hangs below the belly, which is what
//!   a bear's front end does and a pig's does not.
//!
//! - **hump**: Over the shoulder blades: the highest point of the animal and the
//!   first thing over a rise.
//!
//! - **neck**: Short and thick, between the head and the shoulders in height as
//!   well as along: the step that turns the front of the outline into a
//!   slope rather than a head stuck on a wall.
//!
//! - **head**: Heavy and low: its top is under the rump. A head level with the back
//!   is a dog's, and one the height of the body is the capybara.
//!
//! - **snout**: Short, broad and low on the face, standing out of it by nine tenths
//!   of its length and well under the brow: the dished profile a bear's
//!   face has. See `every_animal_has_a_muzzle_in_front_of_its_face`.
//!
//! - **ear left**: Small and round, at the back of the skull and clear of the neck in
//!   front of it. A bear's ears are the one part of it that reads as
//!   friendly, and leaving them off made the head a boulder.
//!
//! - **foreleg left**: Columns, and long ones: seven and a half sixteenths show under the
//!   chest where five did, and that is most of what lifted the animal off
//!   its belly. Hide rather than hoof -- the dark is on the paw.
//!
//! - **foreleg left**: Out to 7.2 where it stood at 7.1, a tenth proud of the body's
//!   side: a tenth is inside what `CLEARANCE` says the depth buffer
//!   can hold apart at a distance, and the legs flickered against the
//!   flanks. The hind legs go the other way, in to 6.8 from 6.9, which
//!   also keeps their inner faces a fifth off the hind paws'.
//!
//! - **hind leg left**: Deeper than the forelegs along the animal, for the haunch, and the
//!   paw longer: a bear walks on the whole of its hind foot.
//!
//! - **tail**: A stub, and it is the whole tail a bear has.
//!
//! ## animals/fowl.bbmodel
//!
//! The ground bird.
//!
//! The smallest model in the game, and it is built as three boxes and a
//! pair of sticks: a body, a head on a short neck, a beak and two legs.
//! What makes it read as a bird rather than as a very small hare is the
//! **fan of tail** behind it and the head carried high and forward.
//!
//! **On the ground its wings are the body's own picture** (see
//! `assets/textures/animals`), which is what a grouse on the ground looks
//! like. **In the air it has a pair of wings**, spread and beating the way the
//! gull's are (`Gait::Wing`, see `GULL` for why a bird has one pair for the
//! ground and one for the air). This bird was written when it only walked
//! and was never given any when `Species::flies` sent it up into the trees:
//! the player watched a grouse flush off a bush as a flying brown box.
//! Short, rounded wings about twice the body across, a game bird's rather
//! than a gull's long ones -- a grouse flies in a burst and a glide, not on
//! a soar.
//!
//! - **body**: A tenth of a sixteenth narrower than it was, so the folded wings stand
//!   a fifth proud of it rather than 0.15, which the depth buffer did not
//!   hold apart at a distance (`model_overlap`); the beak's top a quarter
//!   under the body's rather than a twentieth, and the folded wings a
//!   fifth short of the body's back, for the same reason.
//!
//! - **beak**: A beak is a bird's muzzle, and it wears the muzzle's skins for
//!   that reason: the box in front of the face with the nostrils on
//!   its end. See `every_animal_has_a_muzzle_in_front_of_its_face`.
//!
//! - **folded wing left**: **Folded**: a raised wing down each flank, standing a hair proud of
//!   the body -- 2.55 sixteenths out against the body's 2.4, inside the
//!   0.02 the hit box allows (`the_shared_hit_box_is_the_model_that_is_drawn`)
//!   -- and no face of it in the body's plane, which would flicker.
//!
//! - **wing left**: **Spread**: hinged at the body's flank, two sixteenths in from its
//!   side so the root of the wing is inside the body at the top of a beat,
//!   the inner arm and the rounded hand. Eighteen sixteenths tip to tip.
//!
//! ## animals/gull.bbmodel
//!
//! **The gull**: white, grey-backed and black-tipped, on short pink legs.
//!
//! **Two pairs of wings, and that is the one new thing about it.** Folded, a
//! gull's wings lie along its back and cross over its tail, and that grey
//! back is what a bird on the sand looks like; spread, they are more than
//! twice its length, and that is what a gull in the air *is*. One pair of
//! boxes cannot be both. Three ways were weighed:
//!
//! * **one wing that folds**, swept back about the vertical and rolled about
//!   the length to beat -- two rotations in an order `posed_local` does not
//!   have, a second normal in `world_face_of`, and a folded wing that is a
//!   thin plate seen edge-on from above rather than a back;
//! * **one plate hinged at the shoulder and lowered to fold** -- which hangs
//!   a wing nine sixteenths long off a body three and a half deep, through
//!   the sand;
//! * **two pairs, one drawn at a time** (`Gait::Folded` on the ground,
//!   `Gait::Wing` in the air), which is what is here. It costs four boxes in
//!   the table that are never drawn together, and nothing else.
//!
//! Which pair is chosen by speed (`AIRBORNE_SPEED`), because speed is the
//! one thing the client knows about a flight. The folded tips and the spread
//! ones wear the ear tile, which a bird has no ear for and which the sheet
//! fills with the black tip and its two white mirrors.
//!
//! - **head**: A round white head, carried forward and a little up.
//!
//! - **folded wing left**: **Folded**: the grey of the back, down both upper flanks...
//!
//! - **folded wing left**: Top and back a fifth of a sixteenth past the body's, where they
//!   were a tenth and flickered against it (`model_overlap`).
//!
//! - **folded wingtips**: ...and the black tips crossed over the tail, which is the gull at
//!   thirty blocks.
//!
//! - **wing left**: **Spread**: each wing hinged at two sixteenths out, the grey arm and
//!   the black hand, thirty sixteenths from tip to tip.
//!
//! ## animals/sheep.bbmodel
//!
//! The sheep: a fleece with a face on one end.
//!
//! **The proportions are the animal.** Every other model here is built
//! out from a skeleton -- a body, a head, four legs, and the differences
//! between a hare and a deer are lengths. A sheep is the one that is
//! mostly *not* skeleton: what a player sees is a rectangle of wool with
//! short legs under it and a small dark head sticking out. So the body
//! is drawn nearly as wide as it is long, the legs are half the length
//! they would be on anything else, and the head is the only part not
//! wearing `Fur`.
//!
//! That contrast is doing real work. The wool is what a player is
//! looking for -- see `Species::drops` -- and a flock has to be
//! identifiable across a field at a glance, against grass, from an
//! animal that is not worth the walk.
//!
//! - **body**: The fleece. Broad, deep, and squared off: a round body would read
//!   as a boar, and a boar is the thing that charges.
//!
//! - **body**: **Twelve tall rather than eleven, and the reason is the
//!   texture rather than the shape.** A material skin wears a crop
//!   of its picture sized to the face, and the steps were powers of
//!   two then (they are exact now: `mesh::FINE_UV_BIT`). Eleven
//!   sixteenths snapped down to eight texels while twelve across
//!   snapped up to sixteen, so the fleece's own side carried 1.33
//!   texels per sixteenth one way and 0.73 the other. That is the
//!   very case the old crop's comment warned about: a face whose axes
//!   disagree drops into the filtered path on the denser axis
//!   while the other is still magnified, and the wool came out
//!   stretched against the legs beside it. Reported as "the body
//!   texture is 4x4 where everything else is 16x16".
//!
//!   Twelve by twelve snaps to sixteen by sixteen: 1.33 on both,
//!   which is exactly what a leg three sixteenths wide wearing
//!   four texels already had.
//!
//! - **head**: Carried low and forward -- a sheep grazes, and a head level with
//!   the fleece is what says so. Hide rather than fur: the face is the
//!   one dark part, and it is what makes the animal legible.
//!
//! - **ear left**: Out sideways rather than up. A sheep's ears hang off the side of
//!   its head, and drawn upright they turn the animal into a hare.
//!
//! - **foreleg left**: Short and thin, and only their last few inches are out from under
//!   the fleece -- which is what a sheep's legs look like.
//!
//!   **How much shows is set by the fleece, not by the leg.** A leg
//!   made longer reaches further up inside the body and shows exactly
//!   as much as it did; the number that decides is where the wool
//!   ends. That is why the fix for "a cube with pimples for legs" was
//!   to lift the fleece by an inch and a half and leave the hooves
//!   where they stood: the animal's feet are its contact with the
//!   ground, and the server's box is drawn round its centre, so
//!   dropping them would have sunk the sheep into the field.
//!
//!   Measured against the others, as visible length over leg width:
//!   deer 3.00, hare 1.50, wolf 1.33, boar 0.88, and this sheep was
//!   **0.83** -- broader than it was tall, which is a bump and not a
//!   limb. It is 1.33 now, the wolf's figure, which is what a short
//!   leg under a deep body should read as.
//!
//!   The forelegs stand a fifth of a sixteenth behind the fleece's front,
//!   which their own fronts used to share (`model_overlap`).
//!
//! ## furniture/bed_head.bbmodel
//!
//! The head half of a bed, written with its head toward -z and the seam
//! with the foot half at z sixteen.
//!
//! **Low, and long, and the ends tell you which is which.** A bed read as
//! a bed from across a room is posts, a headboard taller than the
//! footboard, a mattress between side rails, a pillow at one end and a
//! blanket over the rest. Everything under the blanket stops at six and a
//! quarter sixteenths, a hair above the three-eighths the row collides at,
//! so a player standing on the bed stands on the blanket. The boxes that
//! cross into the foot half end exactly on the seam and leave that face
//! open when the foot is there (`furniture_block`): two ends pressed
//! together inside the bed are two faces nobody can see.
//!
//! - **post left**: Two posts, standing past the headboard.
//!
//! - **headboard**: The headboard between them, and the rail capping it -- overhanging
//!   the posts by a quarter, so no face of one lies in a plane of the other.
//!
//! - **side rail left**: The side rails, to the seam.
//!
//! - **slats**: Slats, and the mattress on them.
//!
//! - **pillow**: The pillow.
//!
//! - **blanket**: The blanket from half way down, its edge turned back, hanging over
//!   both rails. **The flat of it stops where the hanging sides begin**
//!   rather than running over them: laid over, its top, its end and its
//!   turned-back edge each shared a plane with a side's for a quarter of
//!   a sixteenth, and a shared plane is two faces the depth buffer picks
//!   between by rounding (`model_overlap`).
//!
//! ## furniture/bed_foot.bbmodel
//!
//! ...and the foot half, from the seam at z zero to the footboard.
//!
//! - **post left**: Lower posts at the foot: a footboard is a board, not a second
//!   headboard, and the difference in height is what says which end the
//!   head goes at.
//!
//! - **side rail left**: Rails, slats and mattress from the seam.
//!
//! - **mattress**: **As wide as the posts stand apart, one to fifteen**, which
//!   is the player's own edit in Blockbench: the mattress rides over the
//!   rails rather than sitting between them. For that the rails stop at the
//!   slats' top, three and a half, and the mattress stops a quarter short of
//!   the rails' end: a wide mattress over rails as tall as it was drew its
//!   end and both sides in the rails' planes, and the bed boiled at its
//!   foot. The head half's mattress is as wide, so the two meet without a
//!   step at the seam.
//!
//! - **blanket**: The blanket, down over both sides and the end. As in the head half,
//!   the flat of it stops at the hanging sides; and the end hangs from a
//!   quarter over the mattress to a quarter under it, a quarter inside the
//!   rails and the blanket's own edges, where it used to share the
//!   mattress's top and bottom, the posts' and the rails' outer faces
//!   (`model_overlap`).
//!
//! ## furniture/straw_bed_head.bbmodel
//!
//! The head half of a straw pallet, written as the bed's is: head toward
//! -z, the seam with the foot half at z sixteen.
//!
//! **Straw and nothing else, and lumpy.** A pallet read as a pallet from
//! across a hut is a long low body of loose grass with a thicker roll at
//! one end and stalks working out of its sides -- no frame, no boards, no
//! blanket, which is what separates it from the bed at a glance. It was a
//! one-cell heap: a mound, a smaller mound and two tufts, and a sleeper's
//! head and feet hung over both ends of it (see `types::is_bed`).
//!
//! The body stops at three and a half sixteenths, under the four its row
//! collides at, and the lumps on it rise a little past that, so a body laid
//! on the collider sinks into the straw rather than floating over it. No
//! two boxes share a plane facing the same way -- the lumps stand on the
//! body, the stalks hang off its sides below its top.
//!
//! - **body**: The body of the pallet, to the seam.
//!
//! - **bolster**: The bolster: straw rolled thick at the head, which is the only thing
//!   that says which end is which.
//!
//! - **lump**: Lumps where the straw has settled unevenly.
//!
//! - **stalks left**: Stalks working loose out of both sides.
//!
//! ## furniture/straw_bed_foot.bbmodel
//!
//! ...and the foot half, from the seam at z zero to a frayed end lower than
//! the body.
//!
//! - **frayed end**: The foot end, where the straw thins out.
//!
//! ## furniture/stool.bbmodel
//!
//! A three-legged stool: a board seat on three poles and one rung. Three
//! legs, because that is the stool its row describes ("three legs need a
//! floor") and because a three-legged stool stands on uneven ground, which
//! is the only ground a camp has. The seat's top is the half block the row
//! collides at, so a sitter's feet are on the seat.
//!
//! ## furniture/chair.bbmodel
//!
//! A chair, written with its front -- where a sitter's knees go -- toward
//! +z and its back at the low z, turned by `bed_quarters` exactly as the
//! bed is, so a chair facing south has its seat edge to the south and its
//! back to the north.
//!
//! **The seat is the stool's, at the half cell the row collides at**, so
//! a sitter's feet rest on the boards that are drawn. Four pole legs, the
//! back two carried on above the seat as the uprights of the back, two
//! board slats between them and a rail capping them at the top of the
//! cell -- a quarter proud of the uprights so no face of one lies in the
//! plane of the other. Three rungs low down, on the sides and the front,
//! which is what stops a four-legged thing reading as a table.
//!
//! The uprights are split at the seat rather than run through it: a pole
//! through a board would be two boxes sharing a volume, and a shared
//! volume is two faces fighting inside the seat wherever the camera
//! catches the seam.
//!
//! - **seat**: The seat.
//!
//! - **front leg left**: Legs under it: the front pair, and the back pair.
//!
//! - **upright left**: The back's uprights, standing on the seat over the back legs.
//!
//! - **slat low**: Two slats between them, and the rail over the top.
//!
//! - **rung left**: Rungs: both sides and the front.
//!
//! ## furniture/table.bbmodel
//!
//! A table: a board top at the six eighths its row collides at, four pole
//! legs, and an apron of boards under the top between them -- the apron is
//! what makes it a table and not a board on sticks.
//!
//! ## furniture/chest.bbmodel
//!
//! A chest, front toward -z, turned to face whoever put it down: a body of
//! boards a sixteenth in from the cell's sides, a lid overhanging it by a
//! quarter with a low crown on top, iron at the four corners, a band low round
//! the body, two straps over the lid and a hasp with its staple across the
//! seam.
//!
//! - **body front**: Boards (`chest`, the row's own side picture), four texels a board, so
//!   the seams run across the front at the height of real planks. **Four
//!   walls a sixteenth thick, not one box**, for "сундук внутри не полый":
//!   with the lid up the body was a solid block with boards painted on its
//!   top. The walls' outer faces are the old body's to the sixteenth, and
//!   a face's picture is the piece under it, so a shut chest is the chest
//!   it was, pixel for pixel.
//!
//! - **floor**, **lining front** (group `inside`): The inside, lit as a shut
//!   space is (`mesh::in_the_dark`) and drawn only while the lid is off it.
//!   The floor is three sixteenths up so the band round the body is buried
//!   in it; the linings are six tenths thick so the corner irons, which
//!   reach half a sixteenth past the walls, are buried in them too -- a
//!   bright iron post in each corner of a dark box -- and stop half a
//!   sixteenth under the rim, so the rim is wood and the inside is dark.
//!
//! - **lid**: A quarter proud of the body all round, so the seam between them is a
//!   shadowed line and not a painted one.
//!
//! - **corner front left**: The corner irons are four tenths proud of the body, and stop a quarter
//!   under the lid: flush with the lid's front they would share its plane,
//!   and iron and wood boil between each other.
//!
//! - **strap left**: Over the lid and down its front and back, **not across its
//!   underside**: the strap was one box from a quarter under the lid to over
//!   it, and a lid stood open showed two iron bars across the inside of it.
//!   Its bottom is a quarter up inside the lid, not a tenth: two downward
//!   faces a tenth apart fight (`model_overlap`).
//!   The two short pieces (`strap left front`, `strap left back`) are what
//!   hangs a quarter under the overhang, so no gap shows at the seam.
//!
//! - **hasp**: The lock plate, a quarter in front of the lid's face and buried in the
//!   body, the only box at that plane in the middle of the front: that is
//!   what `the_mesher_puts_a_turned_chests_hasp_on_the_side_it_faces` looks for.
//!
//! **What was wrong with the one before**: its bands and straps stood a
//! quarter off *every* side at once, so a chest seen from a corner was a
//! cage of iron over a plank, and its boards were `Timber`'s, one seam
//! across the front -- the stool's seat stood on end.
//!
//! ## misc/stake.bbmodel
//!
//! A bundle of stakes stood in the ground (`types::BLOCK_STAKE`, upright):
//! one upright in the middle and four leaning out from it, a quarter of a
//! right angle and a little more, one to each side, each a pole of bark with
//! a point of pale cut wood (`timber`) in two steps. "у кола нету модели, и я
//! хотел шипы, а не одну палку": it was a single stick.
//!
//! - **north shaft**: Leaning by the file's own `rotation`, about x or z and
//!   never both (`mesh::Swing`), about the foot of the pole -- so the foot
//!   stays in the ground and the point goes out. Each spike's point and tip
//!   turn about the same foot as its shaft, or they would come off it.
//!   Short enough that a leaning point stays inside the cell: the stake is
//!   aimed at across its cell (`geometry::block_box_for_aim`), and a point
//!   outside it would be a spike nobody can hit.
//!
//! ## misc/stake_wall.bbmodel
//!
//! Three stakes driven into the wall to the north (-z), points out: one
//! tilted up from the middle, one to each side a little above and below --
//! turned about y, since a spike may only turn about one axis.
//!
//! ## workstations/workbench.bbmodel
//!
//! The joiner's bench (`types::BLOCK_WORKBENCH`), worked from -z. A thick top
//! at the twelve sixteenths the row collides at, four posts, stretchers and a
//! shelf low down -- and **the two things that make a bench a bench and not a
//! table**: a vice on the front edge and a plane lying on the top.
//!
//! - **vice jaw**: The vice's jaw hangs under the front edge, its back against the
//!   top's front face, and its screw stands out of it toward the joiner.
//!
//! - **plane**: A plane on the top, its iron standing out of the sole.
//!
//! ## workstations/sawhorse.bbmodel
//!
//! The sawhorse (`types::BLOCK_SAWHORSE`): a beam on two trestles, a board
//! lying across it and a saw standing in the cut. Worked from -z, the end of
//! the board a joiner stands at.
//!
//! - **crossbar left**: Its top a quarter of a sixteenth under the legs' tops, so the
//!   two never share an upward face (`model_overlap`); the beam sits on it.
//!
//! - **saw blade**: Standing in the kerf at the near end of the board: what makes
//!   a trestle read as a sawhorse and not a low table.
//!
//! ## workstations/honing_stone.bbmodel
//!
//! The honing stone (`types::BLOCK_HONING_STONE`): a slab of dressed stone on
//! a stump, a blade lying on it and a trough of water at its back corner.
//!
//! - **stump**: Bark, because it is a log stood on end: `pole`, as the mason's is.
//!
//! - **trough**: Clear of the stump and the slab's edge, in the corner the
//!   grinder does not stand in.
//!
//! ## workstations/mason_block.bbmodel
//!
//! A slab of dressed stone on a stump: a saddle quern and its rubber on the
//! slab, which is what grinds the grain (`crafting` "quern flour"), and a
//! mallet and a chisel beside them for the ashlar.
//!
//! - **stump**: Bark, because it is a log stood on end: `pole`.
//!
//! ## workstations/potters_wheel.bbmodel
//!
//! A kick wheel: a stone flywheel on the floor, a spindle, the wheel head
//! with a lump of clay being drawn up on it, and a seat behind it for the
//! potter, on the side away from the front.
//!
//! - **clay rim**: The wall of the pot being thrown, a step in from the lump.
//!
//! ## workstations/leather_bench.bbmodel
//!
//! A currier's bench: a table with a hide pinned over it and hanging down the
//! front, a knife and an awl on the hide, and a rolled skin on the shelf.
//!
//! - **hide flap**: The hide hangs over the front edge a half in front of it, and its top
//!   meets the hide on the table without running over it.
//!
//! ## furniture/stall.bbmodel
//!
//! A barter stall: a counter on four legs with a cloth over it and hanging down
//! the front, a shelf under it, and an awning on two posts at the back. Front
//! toward north, the drape's side, where a buyer stands.
//!
//! - **cloth drape**: A sixth of a sixteenth in front of the cloth on the counter, and its
//!   top a twentieth below that cloth's -- the two share no plane facing one way,
//!   which is what `model_overlap` would call a flicker.
//!
//! - **awning**: Held on the back posts alone and reaching over the counter: a
//!   buyer's head is under it, and two posts at the front would stand where
//!   they stand. It stays in its cell (under a roof, a taller one would put its
//!   corners through the thatch).
//!
//! ## misc/drying_rack.bbmodel
//!
//! Draws a drying rack: the frame always, the skin if it has one (see `mesh::rack_block`).
//! Built to the player's photograph: two A-frames of rough poles crossed near
//! the top, and one long pole laid across both crotches.
//!
//! - **west pole a 1**: Each pole is five short straight pieces from its foot to its tip, each
//!   a sixteenth and a half along from the one under it and a quarter to one
//!   side -- a box may not be rotated, so a leaning pole is a stair, and the
//!   quarter is what makes it a crooked stick and not a machined one. A
//!   quarter is also more than `model_overlap`'s clearance, so no two pieces
//!   share a side's plane.
//!
//!   **The feet stand 2.5..13.5 apart and the model stays in its cell.** A
//!   frame taller than a block was allowed and was not taken: a rack under a
//!   roof would put its tips through the thatch, and the mesher's neighbour
//!   culling has already decided the cell above is not there.
//!
//! - **west pole b 1**: The second pole of an A-frame, a quarter apart in x from the first:
//!   lashed, not merged, and never flush.
//!
//! - **ridge west**: The ridge pole, in two pieces a little out of line, nested in the V
//!   of both crossings and overhanging the frames by three quarters at each
//!   end. **This model used to be a frame with no protrusions at all**,
//!   because every tip and overhang of the old rectangle was reported as
//!   breakage; a tip over a crossing is what the photograph is, and a stair
//!   of poles reads as a lashed frame rather than as a glitch.
//!
//! - **hide**: The skin hangs from the ridge between the two frames, its top inside the
//!   ridge so no slot of daylight shows along it.
//!
//!   Its big faces wear [`crate::engine::texture::EXTRA_STRETCHED_HIDE`]
//!   -- a picture *drawn for this slab*, margin, lace holes and all --
//!   and wear it uncropped, exactly the way an animal's face wears its
//!   eye. A crop of a coat tile is the right texture for a coat and the
//!   wrong one for the single face of this model a player actually looks
//!   at: it read as a sheet of cardboard.
//!
//!   **Strips of meat and fish are not drawn**, and that is the block and not
//!   the model: a rack's cell says only *that* something is on it
//!   (`types::RACK_LOADED`, the one spare bit), not what -- so a haunch
//!   drying on the frame is drawn as a skin. See `требования.txt`.
//!
//! Now drawn only for the rack of two by two as it is held, and for a lone
//! cell of it an old save still has; the hide frame that was this rack has a
//! model of its own, below.
//!
//! ## misc/hide_frame.bbmodel
//!
//! A skin laced into a standing frame of four poles, the way hides were
//! dried on the Plains and everywhere else a hide was worked: two uprights
//! planted in the ground, a cross pole lashed top and bottom, and the skin
//! taut in the middle on short cords run from holes round its edge to the
//! poles (see `mesh::rack_block`). Drawn once as a skin pegged out on the
//! ground, and a player found it "strange, like little pegs"; the frame is
//! what the photographs show.
//!
//! **One cell, not a cell and a half.** A frame taller than its cell was
//! allowed -- the palm and the rack of two by two stand out of theirs -- and
//! not taken: a block that draws into the cell above has to be collided and
//! placed against there too, and a hide frame is a thing to put down in a
//! row along a camp, not a structure. The uprights stop a fifth of a
//! sixteenth under the top of the cell.
//!
//! - **west upright**: A leg as well as a side: planted at the floor and standing past
//!   the top cross pole by a sixteenth, as poles cut longer than the frame do.
//!
//! - **top pole**: Lashed in front of the uprights, and **bottom pole** behind them, so the
//!   frame is 6.95..9.05 deep and even about the middle -- which is what lets
//!   one pair of numbers (`types::collision_depth`, 6.75..9.25 with the
//!   lashings) be right for every facing. Both stop half a sixteenth inside
//!   the uprights' outer faces: flush, their end faces fought.
//!
//! - **west lashing top**: The cord bound round a corner, a fifth proud of both poles all
//!   round and narrower than the cross pole, so it reads as a band.
//!
//! - **hide**: Worn uncropped (`Material::StretchedHide`): its picture is drawn for a
//!   slab, laced margin and all. Nine by six and eight tenths, a gap of a
//!   sixteenth and a half to every pole for the lacing to show in. **Cured,
//!   it is the same box in `stretched_leather`** (`Material::cured`,
//!   `types::HIDE_CURED`), so the only thing that changes as a skin dries is
//!   its colour.
//!
//! - **top lace 1**: The lacing, in the `loaded` group: each lace from the skin's edge
//!   to the middle of a pole, leaning the other way from the last, a
//!   zig-zag. It leans under half a sixteenth: at more, two laces meeting at
//!   the pole overlapped in one plane. Sixteen hundredths deep against the
//!   skin's half, so no lace's face lies within `model_overlap`'s clearance
//!   of the skin's.
//!
//! - **loose lace top 1**: The lacing with no skin in it, in the `bare` group: tied to the
//!   poles and hanging. **The `bare` group exists for this**: a frame
//!   drawn as its loaded boxes less the skin was a frame of cords standing
//!   stiff in the air, laced to nothing.

#[cfg(test)]
mod tests {
    #[test]
    fn every_box_a_note_is_about_is_in_its_file() {
        let notes = include_str!("model_notes.rs");
        let mut file: Option<&str> = None;
        for line in notes.lines() {
            if let Some(name) = line.strip_prefix("//! ## ") {
                assert!(crate::embedded::model(name).is_some(), "the notes are about {name}, which is not a model");
                file = Some(name);
            } else if let Some(rest) = line.strip_prefix("//! - **") {
                let part = rest.split("**").next().unwrap();
                let file = file.expect("a note before any file");
                let text = crate::embedded::model(file).unwrap();
                assert!(text.contains(&format!("\"name\": \"{part}\"")) || text.contains(&format!("\"name\":\"{part}\"")), "{file} has no box called \"{part}\", and a note is about it");
            }
        }
        assert!(file.is_some(), "no notes were read");
    }
}
