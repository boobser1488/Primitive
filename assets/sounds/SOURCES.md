# Where the recorded sounds came from

Every file under `assets/sounds/<category>/` is a recording released under
**CC0 1.0 (public domain dedication)**: no attribution, no share-alike, no
non-commercial clause, nothing owed. Crediting the authors below is a
courtesy, not a condition. A file whose page offered CC0 only alongside
something else, or whose archive carried a different licence text than
its page, was left out (`12 wet towel` sounds: the page says CC0, the
archive says MIT/zlib, and MIT asks for its notice to be kept -- so it is
not here).

Every file was cut, summed to mono, trimmed to where the sound is,
peak-normalised and re-encoded as Vorbis (quality 0.3; a high-pass where a recording carried rumble the
measure in `bank` reads as a drum or a note) from the original
download; the originals are not kept in the repository. The table in
`primitive_client/src/audio/recorded.rs` says which game sound each file
is played for.

**Nothing in the game is synthesised any more.** The fire, the rain, the
drips, the three winds, the wings, the bees and every animal's voice were
recipes until the player ordered every sound downloaded and everything
generated deleted; the sections from "Freesound 637523" on are those. They
come from Freesound, and only from sounds whose own page gives the licence
as "Creative Commons 0" -- each page was fetched and read when the file was
downloaded, and a page naming any other licence stopped the download. The
files are Freesound's high-quality previews, because the original upload
needs an account and no account was used. Those were cut the same way, at
24 kHz and Vorbis quality 0.1-0.2; the rain and fire pieces were levelled
by energy instead of by peak and given an equal-power fade at each end,
because the soundscape lays them end over end.

The workshops (`work/`) and the crumbles (`crumble/`) came the same way and
are the last sections here: a hand saw and a plane, a chisel in a quarry, a
ceramics wheel, scissors and a punch through leather, a hammer on an anvil,
and rock, soil, gravel, sand and timber giving way. They were never recipes
-- they are sounds the game simply did not make.

Where no CC0 recording of the animal itself was found, the nearest animal
making the same kind of noise stands in, and its section says what it
really is: a domestic pig and a warthog for the boar, a dog's bark, growl,
yelp and whine for the wolf, rabbits for the hare, chickens, pheasants and
junglefowl for wild fowl, a donkey for the zebra past its one bray, goats
for a wounded antelope.

**The horse has its own recordings now** -- snorts, whinnies and a last
breath under `wild/horse_*`, and hoofbeats at a walk, a trot and a gallop,
on soft ground and on stone or wood, under `step/hoof_*`. It used to be
heard through the zebra's bray and a donkey's. The crickets are
`world/crickets_*`. All of them are the last sections here, fetched the
same way: each page read for "Creative Commons 0" before the download, the
high-quality preview cut, summed to mono, high-passed, and encoded at 16 kHz
(the horse) or 24 kHz (the crickets, whose song is above 3 kHz).

To keep the whole set inside its budget
(`the_recordings_stay_a_modest_download`) the fourth variant of seven
rarely heard cries was dropped when they came in: a boar's wound and its
grunt, a deer's alarm, wound and call, a wolf's alarm and its snarl. Each
of those still has three, and `next_variant` never plays one twice running.

Licence: https://creativecommons.org/publicdomain/zero/1.0/

## Kenney: Impact Sounds

- Author: Kenney (kenney.nl)
- Page: https://kenney.nl/assets/impact-sounds
- Download: https://kenney.nl/media/pages/assets/impact-sounds/87b4ddecda-1677589768/kenney_impact-sounds.zip
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `step/stone_1.ogg` | `Audio/footstep_concrete_000.ogg` |
| `step/stone_2.ogg` | `Audio/footstep_concrete_001.ogg` |
| `step/stone_3.ogg` | `Audio/footstep_concrete_002.ogg` |
| `step/stone_4.ogg` | `Audio/footstep_concrete_003.ogg` |
| `step/stone_5.ogg` | `Audio/footstep_concrete_004.ogg` |
| `step/grass_1.ogg` | `Audio/footstep_grass_000.ogg` |
| `step/grass_2.ogg` | `Audio/footstep_grass_001.ogg` |
| `step/grass_3.ogg` | `Audio/footstep_grass_002.ogg` |
| `step/grass_4.ogg` | `Audio/footstep_grass_003.ogg` |
| `step/grass_5.ogg` | `Audio/footstep_grass_004.ogg` |
| `step/wood_1.ogg` | `Audio/footstep_wood_000.ogg` |
| `step/wood_2.ogg` | `Audio/footstep_wood_001.ogg` |
| `step/wood_3.ogg` | `Audio/footstep_wood_002.ogg` |
| `step/wood_4.ogg` | `Audio/footstep_wood_003.ogg` |
| `step/wood_5.ogg` | `Audio/footstep_wood_004.ogg` |
| `step/snow_1.ogg` | `Audio/footstep_snow_000.ogg` |
| `step/snow_2.ogg` | `Audio/footstep_snow_001.ogg` |
| `step/snow_3.ogg` | `Audio/footstep_snow_002.ogg` |
| `step/snow_4.ogg` | `Audio/footstep_snow_003.ogg` |
| `step/snow_5.ogg` | `Audio/footstep_snow_004.ogg` |
| `step/cloth_1.ogg` | `Audio/footstep_carpet_000.ogg` |
| `step/cloth_2.ogg` | `Audio/footstep_carpet_001.ogg` |
| `step/cloth_3.ogg` | `Audio/footstep_carpet_003.ogg` |
| `dig/stone_1.ogg` | `Audio/impactMining_000.ogg` |
| `dig/stone_2.ogg` | `Audio/impactMining_001.ogg` |
| `dig/stone_3.ogg` | `Audio/impactMining_002.ogg` |
| `dig/stone_4.ogg` | `Audio/impactMining_003.ogg` |
| `dig/stone_5.ogg` | `Audio/impactMining_004.ogg` |
| `dig/wood_1.ogg` | `Audio/impactWood_medium_000.ogg` |
| `dig/wood_2.ogg` | `Audio/impactWood_medium_001.ogg` |
| `dig/wood_3.ogg` | `Audio/impactWood_medium_002.ogg` |
| `dig/wood_4.ogg` | `Audio/impactWood_medium_003.ogg` |
| `dig/wood_5.ogg` | `Audio/impactWood_medium_004.ogg` |
| `dig/metal_1.ogg` | `Audio/impactMetal_light_000.ogg` |
| `dig/metal_2.ogg` | `Audio/impactMetal_light_001.ogg` |
| `dig/metal_3.ogg` | `Audio/impactMetal_light_002.ogg` |
| `dig/metal_4.ogg` | `Audio/impactMetal_light_003.ogg` |
| `dig/metal_5.ogg` | `Audio/impactMetal_light_004.ogg` |
| `dig/glass_1.ogg` | `Audio/impactGlass_light_000.ogg` |
| `dig/glass_2.ogg` | `Audio/impactGlass_light_001.ogg` |
| `dig/glass_3.ogg` | `Audio/impactGlass_light_002.ogg` |
| `dig/glass_4.ogg` | `Audio/impactGlass_light_003.ogg` |
| `dig/glass_5.ogg` | `Audio/impactGlass_light_004.ogg` |
| `dig/ceramic_1.ogg` | `Audio/impactPlate_light_000.ogg` |
| `dig/ceramic_2.ogg` | `Audio/impactPlate_light_001.ogg` |
| `dig/ceramic_3.ogg` | `Audio/impactPlate_light_002.ogg` |
| `dig/ceramic_4.ogg` | `Audio/impactPlate_light_003.ogg` |
| `dig/ceramic_5.ogg` | `Audio/impactPlate_light_004.ogg` |
| `break/wood_1.ogg` | `Audio/impactPlank_medium_000.ogg` |
| `break/wood_2.ogg` | `Audio/impactPlank_medium_001.ogg` |
| `break/wood_3.ogg` | `Audio/impactPlank_medium_002.ogg` |
| `break/wood_4.ogg` | `Audio/impactPlank_medium_003.ogg` |
| `break/wood_5.ogg` | `Audio/impactPlank_medium_004.ogg` |
| `break/metal_1.ogg` | `Audio/impactMetal_heavy_000.ogg` |
| `break/metal_2.ogg` | `Audio/impactMetal_heavy_001.ogg` |
| `break/metal_3.ogg` | `Audio/impactMetal_heavy_002.ogg` |
| `break/metal_4.ogg` | `Audio/impactMetal_heavy_003.ogg` |
| `break/metal_5.ogg` | `Audio/impactMetal_heavy_004.ogg` |
| `break/glass_1.ogg` | `Audio/impactGlass_heavy_000.ogg` |
| `break/glass_2.ogg` | `Audio/impactGlass_heavy_001.ogg` |
| `break/glass_3.ogg` | `Audio/impactGlass_heavy_002.ogg` |
| `break/glass_4.ogg` | `Audio/impactGlass_heavy_003.ogg` |
| `break/glass_5.ogg` | `Audio/impactGlass_heavy_004.ogg` |
| `break/ceramic_1.ogg` | `Audio/impactPlate_heavy_000.ogg` |
| `break/ceramic_2.ogg` | `Audio/impactPlate_heavy_001.ogg` |
| `break/ceramic_3.ogg` | `Audio/impactPlate_heavy_002.ogg` |
| `break/ceramic_4.ogg` | `Audio/impactPlate_heavy_003.ogg` |
| `break/ceramic_5.ogg` | `Audio/impactPlate_heavy_004.ogg` |
| `place/wood_1.ogg` | `Audio/impactWood_heavy_000.ogg` |
| `place/wood_2.ogg` | `Audio/impactWood_heavy_001.ogg` |
| `place/wood_3.ogg` | `Audio/impactWood_heavy_003.ogg` |
| `place/wood_4.ogg` | `Audio/impactWood_heavy_004.ogg` |
| `place/metal_1.ogg` | `Audio/impactMetal_medium_000.ogg` |
| `place/metal_2.ogg` | `Audio/impactMetal_medium_001.ogg` |
| `place/metal_3.ogg` | `Audio/impactMetal_medium_002.ogg` |
| `place/metal_4.ogg` | `Audio/impactMetal_medium_003.ogg` |
| `place/metal_5.ogg` | `Audio/impactMetal_medium_004.ogg` |
| `place/glass_1.ogg` | `Audio/impactGlass_medium_000.ogg` |
| `place/glass_2.ogg` | `Audio/impactGlass_medium_001.ogg` |
| `place/glass_3.ogg` | `Audio/impactGlass_medium_002.ogg` |
| `place/glass_4.ogg` | `Audio/impactGlass_medium_003.ogg` |
| `place/glass_5.ogg` | `Audio/impactGlass_medium_004.ogg` |
| `place/ceramic_1.ogg` | `Audio/impactPlate_medium_000.ogg` |
| `place/ceramic_2.ogg` | `Audio/impactPlate_medium_001.ogg` |
| `place/ceramic_3.ogg` | `Audio/impactPlate_medium_002.ogg` |
| `place/ceramic_4.ogg` | `Audio/impactPlate_medium_003.ogg` |
| `place/ceramic_5.ogg` | `Audio/impactPlate_medium_004.ogg` |
| `player/hurt_1.ogg` | `Audio/impactPunch_heavy_000.ogg` |
| `player/hurt_2.ogg` | `Audio/impactPunch_heavy_001.ogg` |
| `player/hurt_3.ogg` | `Audio/impactPunch_heavy_002.ogg` |
| `player/hurt_4.ogg` | `Audio/impactPunch_heavy_003.ogg` |
| `player/hurt_5.ogg` | `Audio/impactPunch_heavy_004.ogg` |
| `hand/hit_1.ogg` | `Audio/impactPunch_medium_000.ogg` |
| `hand/hit_2.ogg` | `Audio/impactPunch_medium_001.ogg` |
| `hand/hit_3.ogg` | `Audio/impactPunch_medium_002.ogg` |
| `hand/hit_4.ogg` | `Audio/impactPunch_medium_003.ogg` |
| `hand/hit_5.ogg` | `Audio/impactPunch_medium_004.ogg` |
| `ui/click_1.ogg` | `Audio/impactWood_light_000.ogg` |
| `ui/click_2.ogg` | `Audio/impactWood_light_001.ogg` |
| `ui/click_3.ogg` | `Audio/impactWood_light_002.ogg` |
| `ui/click_4.ogg` | `Audio/impactWood_light_003.ogg` |
| `ui/click_5.ogg` | `Audio/impactWood_light_004.ogg` |

## Kenney: RPG Audio

- Author: Kenney (kenney.nl)
- Page: https://kenney.nl/assets/rpg-audio
- Download: https://kenney.nl/media/pages/assets/rpg-audio/8e99002d76-1677590336/kenney_rpg-audio.zip
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `step/dirt_1.ogg` | `Audio/footstep00.ogg` |
| `step/dirt_2.ogg` | `Audio/footstep01.ogg` |
| `step/dirt_3.ogg` | `Audio/footstep03.ogg` |
| `step/dirt_4.ogg` | `Audio/footstep06.ogg` |
| `step/dirt_5.ogg` | `Audio/footstep07.ogg` |
| `step/dirt_6.ogg` | `Audio/footstep08.ogg` |
| `dig/dirt_1.ogg` | `Audio/footstep02.ogg` |
| `dig/dirt_2.ogg` | `Audio/footstep04.ogg` |
| `dig/dirt_3.ogg` | `Audio/footstep05.ogg` |
| `dig/dirt_4.ogg` | `Audio/footstep09.ogg` |
| `dig/cloth_1.ogg` | `Audio/cloth1.ogg` |
| `dig/cloth_2.ogg` | `Audio/cloth2.ogg` |
| `dig/cloth_3.ogg` | `Audio/cloth3.ogg` |
| `dig/cloth_4.ogg` | `Audio/cloth4.ogg` |
| `item/pickup_1.ogg` | `Audio/handleSmallLeather.ogg` |
| `item/pickup_2.ogg` | `Audio/handleSmallLeather2.ogg` |
| `item/pickup_3.ogg` | `Audio/cloth2.ogg` |
| `item/drop_1.ogg` | `Audio/dropLeather.ogg` |
| `item/drop_2.ogg` | `Audio/bookPlace1.ogg` |
| `item/drop_3.ogg` | `Audio/bookPlace2.ogg` |
| `item/drop_4.ogg` | `Audio/bookPlace3.ogg` |
| `item/equip_1.ogg` | `Audio/clothBelt.ogg` |
| `item/equip_2.ogg` | `Audio/clothBelt2.ogg` |
| `item/equip_3.ogg` | `Audio/beltHandle1.ogg` |
| `item/equip_4.ogg` | `Audio/beltHandle2.ogg` |
| `item/craft_4.ogg` | `Audio/chop.ogg` |
| `chest/open_2.ogg` | `Audio/doorOpen_1.ogg` |
| `chest/open_3.ogg` | `Audio/doorOpen_2.ogg` |
| `chest/close_1.ogg` | `Audio/doorClose_1.ogg` |
| `chest/close_2.ogg` | `Audio/doorClose_2.ogg` |
| `chest/close_3.ogg` | `Audio/doorClose_3.ogg` |
| `chest/close_4.ogg` | `Audio/doorClose_4.ogg` |
| `ui/back_1.ogg` | `Audio/bookClose.ogg` |
| `ui/back_2.ogg` | `Audio/bookFlip3.ogg` |
| `ui/message_1.ogg` | `Audio/bookFlip1.ogg` |
| `ui/message_2.ogg` | `Audio/bookFlip2.ogg` |

## 100 CC0 SFX

- Author: rubberduck
- Page: https://opengameart.org/content/100-cc0-sfx
- Download: https://opengameart.org/sites/default/files/100-CC0-SFX_0.zip
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `chest/open_1.ogg` | `wooded_box_open.ogg` |
| `chest/open_4.ogg` | `wooden_02.ogg` |
| `chest/open_5.ogg` | `wooden_03.ogg` |
| `ui/message_3.ogg` | `paper_01.ogg` |

## 100 CC0 SFX #2

- Author: rubberduck
- Page: https://opengameart.org/content/100-cc0-sfx-2
- Download: https://opengameart.org/sites/default/files/sfx_100_v2.zip
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `break/dirt_3.ogg` | `sfx100v2_footstep_01.ogg` |
| `break/dirt_4.ogg` | `sfx100v2_footstep_02.ogg` |
| `break/gravel_1.ogg` | `sfx100v2_stones_01.ogg` |
| `break/gravel_2.ogg` | `sfx100v2_stones_02.ogg` |
| `break/gravel_3.ogg` | `sfx100v2_stones_03.ogg` |
| `world/thunder_1.ogg` | `sfx100v2_thunder_01.ogg` |

## 80 CC0 RPG SFX

- Author: rubberduck
- Page: https://opengameart.org/content/80-cc0-rpg-sfx
- Download: https://opengameart.org/sites/default/files/80-CC0-RPG-SFX_0.zip
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `dig/gravel_1.ogg` | `x/stones_01.ogg` |
| `dig/gravel_2.ogg` | `x/stones_02.ogg` |
| `dig/gravel_3.ogg` | `x/stones_03.ogg` |
| `dig/gravel_4.ogg` | `x/stones_04.ogg` |
| `item/craft_1.ogg` | `x/item_wood_01.ogg` |
| `item/craft_2.ogg` | `x/item_wood_02.ogg` |
| `item/craft_3.ogg` | `x/item_wood_03.ogg` |
| `chest/open_6.ogg` | `x/item_wood_02.ogg` |

## 40 CC0 water / splash / slime SFX

- Author: rubberduck
- Page: https://opengameart.org/content/40-cc0-water-splash-slime-sfx
- Download: https://opengameart.org/sites/default/files/water-splash-slime-sfx.zip
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `dig/liquid_1.ogg` | `x/splash_09.ogg` |
| `dig/liquid_2.ogg` | `x/splash_10.ogg` |
| `player/bubble_4.ogg` | `x/bubble_02.ogg` |

## 6 short water splashes

- Author: qubodup, from pdsounds.org (public domain)
- Page: https://opengameart.org/content/6-short-water-splashes
- Download: https://opengameart.org/sites/default/files/ezwa-water_splash.7z
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `dig/liquid_3.ogg` | `x/ezwa-water_splash/water_splash-05.flac` |
| `player/splash_4.ogg` | `x/ezwa-water_splash/water_splash-01.flac` |

## Bubble sound effects

- Author: bmaczero
- Page: https://opengameart.org/content/bubble-sound-effects
- Download: https://opengameart.org/sites/default/files/bubbles-single1.wav (...2, ...3)
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `player/bubble_1.ogg` | `bubbles-single1.wav` |
| `player/bubble_2.ogg` | `bubbles-single2.wav` |
| `player/bubble_3.ogg` | `bubbles-single3.wav` |

## Liquid, bottle & drink set

- Author: qubodup
- Page: https://opengameart.org/content/liquid-bottle-drink-set
- Download: https://opengameart.org/sites/default/files/qubodup-bottle-and-drink-sounds.7z
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `player/drink_1.ogg` | `x/qubodup-bottle-and-drink-sounds/swallow-01.flac` |
| `player/drink_2.ogg` | `x/qubodup-bottle-and-drink-sounds/swallow-03.flac` |
| `player/drink_3.ogg` | `x/qubodup-bottle-and-drink-sounds/swallow-04.flac` |

## 7 eating crunches

- Author: StarNinjas
- Page: https://opengameart.org/content/7-eating-crunches
- Download: https://opengameart.org/sites/default/files/crunch_-_tito.zip
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `player/eat_1.ogg` | `x/crunch.1.ogg` |
| `player/eat_2.ogg` | `x/crunch.2.ogg` |
| `player/eat_3.ogg` | `x/crunch.3.ogg` |
| `player/eat_4.ogg` | `x/crunch.4.ogg` |
| `player/eat_5.ogg` | `x/crunch.5.ogg` |
| `player/eat_6.ogg` | `x/crunch.6.ogg` |
| `player/eat_7.ogg` | `x/crunch.7.ogg` |

## 42 snow and gravel footsteps

- Author: Corsica_S, cut by qubodup (released as CC0 with the recordist's permission)
- Page: https://opengameart.org/content/42-snow-and-gravel-footsteps
- Download: https://opengameart.org/sites/default/files/corsica_s-walking_in_snow.7z
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `step/gravel_1.ogg` | `x/Corsica_S-Walking_in_Snow/Corsica_S-Walking_on_snow_covered_gravel_04.flac` |
| `step/gravel_2.ogg` | `x/Corsica_S-Walking_in_Snow/Corsica_S-Walking_on_snow_covered_gravel_05.flac` |
| `step/gravel_3.ogg` | `x/Corsica_S-Walking_in_Snow/Corsica_S-Walking_on_snow_covered_gravel_08.flac` |
| `step/gravel_4.ogg` | `x/Corsica_S-Walking_in_Snow/Corsica_S-Walking_on_snow_covered_gravel_09.flac` |
| `step/gravel_5.ogg` | `x/Corsica_S-Walking_in_Snow/Corsica_S-Walking_on_snow_covered_gravel_10.flac` |
| `step/gravel_6.ogg` | `x/Corsica_S-Walking_in_Snow/Corsica_S-Walking_on_snow_covered_gravel_12.flac` |

## Fantozzi's footsteps (grass/sand & stone)

- Author: Fantozzi, submitted by qubodup
- Page: https://opengameart.org/content/fantozzis-footsteps-grasssand-stone
- Download: https://opengameart.org/sites/default/files/Fantozzi-footsteps.7z
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `step/sand_1.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-SandL1.flac` |
| `step/sand_2.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-SandL2.flac` |
| `step/sand_3.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-SandL3.flac` |
| `step/sand_4.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-SandR1.flac` |
| `step/sand_5.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-SandR2.flac` |
| `step/sand_6.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-SandR3.flac` |
| `place/stone_1.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-StoneL1.flac` |
| `place/stone_2.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-StoneL2.flac` |
| `place/stone_3.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-StoneL3.flac` |
| `place/stone_4.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-StoneR1.flac` |
| `place/stone_5.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-StoneR2.flac` |
| `place/stone_6.ogg` | `x/Fantozzi-footsteps/flac/Fantozzi-StoneR3.flac` |

## 20 rustles (dry leaves)

- Author: qubodup
- Page: https://opengameart.org/content/20-rustles-dry-leaves
- Download: https://opengameart.org/sites/default/files/qubodup-rustle.7z
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `dig/grass_1.ogg` | `x/rustle/rustle02.flac` |
| `dig/grass_2.ogg` | `x/rustle/rustle09.flac` |
| `dig/grass_3.ogg` | `x/rustle/rustle10.flac` |
| `dig/grass_4.ogg` | `x/rustle/rustle12.flac` |
| `dig/grass_5.ogg` | `x/rustle/rustle05.flac` |
| `break/grass_1.ogg` | `x/rustle/rustle01.flac` |
| `break/grass_2.ogg` | `x/rustle/rustle03.flac` |
| `break/grass_3.ogg` | `x/rustle/rustle08.flac` |
| `break/grass_4.ogg` | `x/rustle/rustle11.flac` |
| `break/grass_5.ogg` | `x/rustle/rustle13.flac` |

## 5 break / crunch impacts

- Author: Independent.nu, submitted by qubodup
- Page: https://opengameart.org/content/5-break-crunch-impacts
- Download: https://opengameart.org/sites/default/files/independent_nu_ljudbank-break_crunch_impact.7z
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `break/stone_1.ogg` | `x/impcrunch/impactcrunch01.mp3.flac` |
| `break/stone_2.ogg` | `x/impcrunch/impactcrunch02.mp3.flac` |
| `break/stone_3.ogg` | `x/impcrunch/impactcrunch03.mp3.flac` |
| `break/stone_4.ogg` | `x/impcrunch/impactcrunch04.mp3.flac` |
| `break/stone_5.ogg` | `x/impcrunch/impactcrunch05.mp3.flac` |

## 37 hits / punches

- Author: Independent.nu, submitted by qubodup
- Page: https://opengameart.org/content/37-hitspunches
- Download: https://opengameart.org/sites/default/files/independent_nu_ljudbank-hits_and_punches.7z
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `player/death_1.ogg` | `x/hits/hit33.mp3.flac` |
| `player/death_2.ogg` | `x/hits/hit37.mp3.flac` |

## Swishes sound pack

- Author: artisticdude
- Page: https://opengameart.org/content/swishes-sound-pack
- Download: https://opengameart.org/sites/default/files/swishes.zip
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `hand/swing_1.ogg` | `x/swishes/swish-1.wav` |
| `hand/swing_2.ogg` | `x/swishes/swish-4.wav` |
| `hand/swing_3.ogg` | `x/swishes/swish-5.wav` |
| `hand/swing_4.ogg` | `x/swishes/swish-7.wav` |
| `hand/swing_5.ogg` | `x/swishes/swish-9.wav` |

## Catching fire

- Author: themightyglider
- Page: https://opengameart.org/content/catching-fire
- Download: https://opengameart.org/sites/default/files/flame_0.ogg
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `fire/ignite_1.ogg` | `flame_0.ogg` |

## Fire crackling

- Author: AntumDeluge
- Page: https://opengameart.org/content/fire-crackling
- Download: https://opengameart.org/sites/default/files/fire-1_0.ogg
- Licence: CC0 1.0

**No longer shipped.** The player asked for a different fire -- this one is
a string of separate snaps -- and the fire was a recipe for a while, until
every recipe was deleted; the fire now is "Freesound 637523" below, cut
into pieces that happen to reuse these names. The four cuts below were
taken out of `assets/sounds` rather than deleted; the entry stays so where
they came from is not lost.

| file | cut from |
|---|---|
| fire/crackle_1.ogg (retired) | `fire-1_0.ogg` |
| fire/crackle_2.ogg (retired) | `fire-1_0.ogg` |
| fire/crackle_3.ogg (retired) | `fire-1_0.ogg` |
| fire/crackle_4.ogg (retired) | `fire-1_0.ogg` |

## Solo seagull sound effects

- Author: rango-mango
- Page: https://opengameart.org/content/solo-seagull-sound-effects
- Download: https://opengameart.org/sites/default/files/Seagull%20Ambient%202.wav (...3, ...4, ...5)
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `wild/gull_1.ogg` | `Seagull_Ambient_2.wav` |
| `wild/gull_2.ogg` | `Seagull_Ambient_3.wav` |
| `wild/gull_3.ogg` | `Seagull_Ambient_4.wav` |
| `wild/gull_4.ogg` | `Seagull_Ambient_5.wav` |

## Frog croaks

- Author: ezduzziteh
- Page: https://opengameart.org/content/frog-croaks
- Download: https://opengameart.org/sites/default/files/croak_01_0.mp3 (croak_02 ... croak_04)
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `wild/frog_1.ogg` | `croak_01_0.mp3` |
| `wild/frog_2.ogg` | `croak_02.mp3` |
| `wild/frog_3.ogg` | `croak_03.mp3` |
| `wild/frog_4.ogg` | `croak_04.mp3` |

## Different steps on wood, stone, leaves, gravel and mud

- Author: TinyWorlds
- Page: https://opengameart.org/content/different-steps-on-wood-stone-leaves-gravel-and-mud
- Download: https://opengameart.org/sites/default/files/%5Bkdd%5DDifferentSteps_0.zip
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `break/dirt_2.ogg` | `mud02.ogg` |

## Shovel sound

- Author: themightyglider
- Page: https://opengameart.org/content/shovel-sound
- Download: https://opengameart.org/sites/default/files/shovel_0.ogg
- Licence: CC0 1.0

| file | cut from |
|---|---|
| `break/dirt_1.ogg` | `shovel_0.ogg` |

## Freesound 637523: fire small campfire crackling short +air tone.flac

- Author: kyles (freesound.org)
- Page: https://freesound.org/people/kyles/sounds/637523/
- Download: https://cdn.freesound.org/previews/637/637523_612689-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `fire/crackle_1.ogg` | 15.0-17.4 s |
| `fire/crackle_2.ogg` | 6.6-9.0 s |
| `fire/crackle_3.ogg` | 12.6-15.0 s |

## Freesound 850008: Antelope - Blackbuck; Snort, Single

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/850008/
- Download: https://cdn.freesound.org/previews/850/850008_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/antelope_alarm_1.ogg` | 0.05-0.25 s |

## Freesound 839917: Antelope - Blackbuck, Two Snorts

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/839917/
- Download: https://cdn.freesound.org/previews/839/839917_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/antelope_alarm_2.ogg` | 0.24-0.40 s |
| `wild/antelope_alarm_3.ogg` | 1.65-1.89 s |

## Freesound 825399: Goats - Billy Goat Bleat

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/825399/
- Download: https://cdn.freesound.org/previews/825/825399_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/antelope_death_1.ogg` | 0.20-1.75 s |

## Freesound 792534: Goats - Kid Bleat, Close Perspective

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/792534/
- Download: https://cdn.freesound.org/previews/792/792534_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/antelope_hurt_1.ogg` | 0.06-0.81 s |
| `wild/antelope_death_2.ogg` | 0.06-0.81 s |

## Freesound 777755: Goats - Goat Bleat; Close Perspective

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/777755/
- Download: https://cdn.freesound.org/previews/777/777755_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/antelope_hurt_2.ogg` | 0.04-0.86 s |

## Freesound 839527: Goats - Kid Bleat, Medium Perspective

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/839527/
- Download: https://cdn.freesound.org/previews/839/839527_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/antelope_hurt_3.ogg` | 0.08-0.73 s |

## Freesound 838368: Antelope - Scimitar Oryx; Baby Grunt, Close Perspective

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/838368/
- Download: https://cdn.freesound.org/previews/838/838368_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/antelope_idle_1.ogg` | 0.05-0.63 s |

## Freesound 838456: Antelope - Scimitar Oryx; Baby, Nasal Snort and Grunt, Close Perspective

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/838456/
- Download: https://cdn.freesound.org/previews/838/838456_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/antelope_idle_2.ogg` | 0.74-1.19 s |
| `wild/antelope_idle_3.ogg` | 4.98-5.26 s |

## Freesound 519600: running grizzly.mp3

- Author: Nivatius (freesound.org)
- Page: https://freesound.org/people/Nivatius/sounds/519600/
- Download: https://cdn.freesound.org/previews/519/519600_7143328-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/bear_alarm_2.ogg` | 8.08-9.38 s |
| `wild/bear_hurt_2.ogg` | 7.00-7.90 s |

## Freesound 519598: huf grizzly

- Author: Nivatius (freesound.org)
- Page: https://freesound.org/people/Nivatius/sounds/519598/
- Download: https://cdn.freesound.org/previews/519/519598_7143328-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/bear_idle_1.ogg` | 0.05-0.40 s |

## Freesound 519597: huf grizzly bear

- Author: Nivatius (freesound.org)
- Page: https://freesound.org/people/Nivatius/sounds/519597/
- Download: https://cdn.freesound.org/previews/519/519597_7143328-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/bear_idle_2.ogg` | 0.07-0.32 s |

## Freesound 519596: move and growl grizzly bear

- Author: Nivatius (freesound.org)
- Page: https://freesound.org/people/Nivatius/sounds/519596/
- Download: https://cdn.freesound.org/previews/519/519596_7143328-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/bear_idle_3.ogg` | 4.28-4.93 s |
| `wild/bear_alarm_1.ogg` | 2.49-2.94 s |
| `wild/bear_alarm_3.ogg` | 8.10-8.75 s |

## Freesound 763026: Bear Angry Growl

- Author: celldroid (freesound.org)
- Page: https://freesound.org/people/celldroid/sounds/763026/
- Download: https://cdn.freesound.org/previews/763/763026_1764719-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/bear_threat_1.ogg` | 1.75-4.75 s |
| `wild/bear_hurt_3.ogg` | 3.60-4.70 s |
| `wild/bear_death_2.ogg` | 2.30-4.70 s |

## Freesound 439441: bear_mad.wav

- Author: Asteroiderer (freesound.org)
- Page: https://freesound.org/people/Asteroiderer/sounds/439441/
- Download: https://cdn.freesound.org/previews/439/439441_7346113-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/bear_threat_2.ogg` | 0.00-2.60 s |
| `wild/bear_threat_3.ogg` | 2.90-5.50 s |
| `wild/bear_hurt_1.ogg` | 5.60-6.80 s |
| `wild/bear_death_1.ogg` | 6.30-8.86 s |

## Freesound 521364: BeesShortBurst.wav

- Author: taylordonj (freesound.org)
- Page: https://freesound.org/people/taylordonj/sounds/521364/
- Download: https://cdn.freesound.org/previews/521/521364_21674-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/bees_1.ogg` | 0.0-1.3 s |
| `wild/bees_2.ogg` | 1.3-2.6 s |
| `wild/bees_3.ogg` | 3.9-5.2 s |
| `wild/bees_4.ogg` | 5.2-6.5 s |

## Freesound 850012: Swine - Warthog; Grunting and Squealing

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/850012/
- Download: https://cdn.freesound.org/previews/850/850012_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/boar_alarm_1.ogg` | 0.10-0.58 s |
| `wild/boar_alarm_2.ogg` | 2.10-2.95 s |
| `wild/boar_alarm_3.ogg` | 3.80-4.90 s |

## Freesound 842313: Swine - Pig; Dinosaur-like Grunt

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/842313/
- Download: https://cdn.freesound.org/previews/842/842313_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/boar_death_3.ogg` | 0.00-1.75 s |

## Freesound 344972: Pig in slop squealing

- Author: mrmunk (freesound.org)
- Page: https://freesound.org/people/mrmunk/sounds/344972/
- Download: https://cdn.freesound.org/previews/344/344972_1255966-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/boar_hurt_1.ogg` | 0.93-1.23 s |
| `wild/boar_hurt_2.ogg` | 8.25-8.50 s |

## Freesound 260640: Pig Squealing.mp3

- Author: TheAcidRomance (freesound.org)
- Page: https://freesound.org/people/TheAcidRomance/sounds/260640/
- Download: https://cdn.freesound.org/previews/260/260640_2362629-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/boar_hurt_3.ogg` | 3.83-4.38 s |
| `wild/boar_death_1.ogg` | 0.12-2.42 s |
| `wild/boar_death_2.ogg` | 11.15-12.55 s |

## Freesound 778119: Swine - Pig; Various Pig Grunts, Close Perspective

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/778119/
- Download: https://cdn.freesound.org/previews/778/778119_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/boar_idle_1.ogg` | 0.40-0.70 s |
| `wild/boar_idle_2.ogg` | 10.50-10.92 s |
| `wild/boar_idle_3.ogg` | 11.70-12.10 s |

## Freesound 352698: Angry Pig Oinking

- Author: Jofae (freesound.org)
- Page: https://freesound.org/people/Jofae/sounds/352698/
- Download: https://cdn.freesound.org/previews/352/352698_6512973-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/boar_threat_1.ogg` | 0.05-0.75 s |
| `wild/boar_threat_2.ogg` | 1.58-2.03 s |
| `wild/boar_threat_3.ogg` | 2.30-3.25 s |

## Freesound 507093: Fish Splashing Release 2.wav

- Author: paulprit (freesound.org)
- Page: https://freesound.org/people/paulprit/sounds/507093/
- Download: https://cdn.freesound.org/previews/507/507093_8682843-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/cod_alarm_1.ogg` | 0.35-0.95 s |
| `wild/cod_alarm_2.ogg` | 1.00-1.55 s |
| `wild/cod_hurt_2.ogg` | 1.50-2.40 s |
| `wild/cod_death_1.ogg` | 0.35-2.50 s |

## Freesound 696776: Deer_Bark_5_s

- Author: ferventtorpor (freesound.org)
- Page: https://freesound.org/people/ferventtorpor/sounds/696776/
- Download: https://cdn.freesound.org/previews/696/696776_9391615-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/deer_alarm_1.ogg` | 0.00-0.85 s |

## Freesound 569926: Deer barking.m4a

- Author: Spamanator (freesound.org)
- Page: https://freesound.org/people/Spamanator/sounds/569926/
- Download: https://cdn.freesound.org/previews/569/569926_12833977-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/deer_alarm_2.ogg` | 2.95-3.60 s |
| `wild/deer_alarm_3.ogg` | 8.50-9.20 s |

## Freesound 653532: Deer rut - Brame Cerf .wav

- Author: L.Finck (freesound.org)
- Page: https://freesound.org/people/L.Finck/sounds/653532/
- Download: https://cdn.freesound.org/previews/653/653532_14042309-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/deer_death_3.ogg` | 19.60-21.15 s |

## Freesound 351113: Deer Meewing 01.wav

- Author: msantoro11 (freesound.org)
- Page: https://freesound.org/people/msantoro11/sounds/351113/
- Download: https://cdn.freesound.org/previews/351/351113_4790684-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/deer_idle_1.ogg` | 0.50-0.90 s |
| `wild/deer_idle_2.ogg` | 4.50-5.15 s |
| `wild/deer_idle_3.ogg` | 9.35-9.75 s |
| `wild/deer_hurt_1.ogg` | 1.75-2.10 s |
| `wild/deer_hurt_2.ogg` | 6.45-6.90 s |
| `wild/deer_hurt_3.ogg` | 13.65-14.05 s |
| `wild/deer_death_1.ogg` | 5.30-6.20 s |
| `wild/deer_death_2.ogg` | 11.80-12.50 s |

## Freesound 679389: Fish flopping over on sand

- Author: adviseme333 (freesound.org)
- Page: https://freesound.org/people/adviseme333/sounds/679389/
- Download: https://cdn.freesound.org/previews/679/679389_14805886-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/fish_alarm_1.ogg` | 0.55-0.85 s |
| `wild/fish_alarm_2.ogg` | 3.25-3.45 s |

## Freesound 507094: Fish Splashing Release 1.wav

- Author: paulprit (freesound.org)
- Page: https://freesound.org/people/paulprit/sounds/507094/
- Download: https://cdn.freesound.org/previews/507/507094_8682843-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/fish_alarm_3.ogg` | 0.20-0.70 s |
| `wild/fish_hurt_1.ogg` | 0.70-1.40 s |
| `wild/fish_death_1.ogg` | 0.20-1.90 s |

## Freesound 570208: Fish Flopping.wav

- Author: RatBird (freesound.org)
- Page: https://freesound.org/people/RatBird/sounds/570208/
- Download: https://cdn.freesound.org/previews/570/570208_12508711-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/fish_hurt_2.ogg` | 0.00-1.00 s |
| `wild/fish_death_2.ogg` | 1.00-2.50 s |
| `wild/cod_hurt_1.ogg` | 2.50-3.40 s |
| `wild/cod_death_2.ogg` | 2.40-4.05 s |

## Freesound 316921: Chicken Alarm Call full with Occasional bird sound

- Author: Rudmer_Rotteveel (freesound.org)
- Page: https://freesound.org/people/Rudmer_Rotteveel/sounds/316921/
- Download: https://cdn.freesound.org/previews/316/316921_4921277-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/fowl_alarm_1.ogg` | 1.03-1.53 s |
| `wild/fowl_alarm_2.ogg` | 6.60-7.35 s |
| `wild/fowl_alarm_3.ogg` | 8.93-9.78 s |
| `wild/fowl_hurt_2.ogg` | 0.50-0.92 s |
| `wild/fowl_hurt_3.ogg` | 2.50-2.92 s |
| `wild/fowl_death_2.ogg` | 4.18-5.13 s |

## Freesound 316920: Chicken Single Alarm Call

- Author: Rudmer_Rotteveel (freesound.org)
- Page: https://freesound.org/people/Rudmer_Rotteveel/sounds/316920/
- Download: https://cdn.freesound.org/previews/316/316920_4921277-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/fowl_death_1.ogg` | 0.02-0.92 s |

## Freesound 864292: Fowl - Indian Peafowl; Short Squawk, Reverberant

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/864292/
- Download: https://cdn.freesound.org/previews/864/864292_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/fowl_hurt_1.ogg` | 0.00-0.50 s |

## Freesound 850007: Fowl - Ceylon Junglefowl

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/850007/
- Download: https://cdn.freesound.org/previews/850/850007_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/fowl_idle_1.ogg` | 0.28-1.63 s |

## Freesound 849184: Fowl - Common Pheasant, Crow

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/849184/
- Download: https://cdn.freesound.org/previews/849/849184_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/fowl_idle_2.ogg` | 0.52-1.24 s |

## Freesound 867600: Fowl - Erckel's Spurfowl; Bark-like Clucks

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/867600/
- Download: https://cdn.freesound.org/previews/867/867600_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/fowl_idle_3.ogg` | 0.55-2.10 s |

## Freesound 468836: 16_2_Gaviotas_peleando.wav

- Author: ChristianAnd (freesound.org)
- Page: https://freesound.org/people/ChristianAnd/sounds/468836/
- Download: https://cdn.freesound.org/previews/468/468836_9934055-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/gull_death_2.ogg` | 0.30-2.60 s |

## Freesound 836859: Seabirds - Ring-billed Gull; Single Call

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/836859/
- Download: https://cdn.freesound.org/previews/836/836859_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/gull_hurt_1.ogg` | 0.50-1.25 s |

## Freesound 861350: Seabirds - Ring-billed Gull; Single Call, Take 2

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/861350/
- Download: https://cdn.freesound.org/previews/861/861350_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/gull_hurt_2.ogg` | 0.05-0.77 s |

## Freesound 468835: 16_Gaviotas_Peleando.wav

- Author: ChristianAnd (freesound.org)
- Page: https://freesound.org/people/ChristianAnd/sounds/468835/
- Download: https://cdn.freesound.org/previews/468/468835_9934055-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/gull_hurt_3.ogg` | 0.18-1.28 s |
| `wild/gull_death_1.ogg` | 3.00-5.45 s |

## Freesound 386007: Rabbit Thump on Soil

- Author: kessir (freesound.org)
- Page: https://freesound.org/people/kessir/sounds/386007/
- Download: https://cdn.freesound.org/previews/386/386007_4521595-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/hare_alarm_1.ogg` | 0.55-0.90 s |
| `wild/hare_alarm_2.ogg` | 3.80-4.15 s |

## Freesound 372076: Rabbit snarls and growls

- Author: kessir (freesound.org)
- Page: https://freesound.org/people/kessir/sounds/372076/
- Download: https://cdn.freesound.org/previews/372/372076_4521595-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/hare_death_3.ogg` | 10.50-10.90 s |

## Freesound 372075: Rabbit oinks and squeaks

- Author: kessir (freesound.org)
- Page: https://freesound.org/people/kessir/sounds/372075/
- Download: https://cdn.freesound.org/previews/372/372075_4521595-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/hare_hurt_1.ogg` | 11.85-12.10 s |
| `wild/hare_hurt_2.ogg` | 20.40-20.55 s |
| `wild/hare_hurt_3.ogg` | 22.55-22.70 s |
| `wild/hare_death_1.ogg` | 20.20-21.40 s |
| `wild/hare_death_2.ogg` | 22.05-23.00 s |

## Freesound 270383: lion_growls.wav

- Author: stratcat322 (freesound.org)
- Page: https://freesound.org/people/stratcat322/sounds/270383/
- Download: https://cdn.freesound.org/previews/270/270383_1808829-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/lion_alarm_1.ogg` | 0.48-1.14 s |
| `wild/lion_alarm_2.ogg` | 1.29-2.11 s |
| `wild/lion_hurt_2.ogg` | 3.48-4.40 s |

## Freesound 699928: Lion Grunt.wav

- Author: 8bitmyketison (freesound.org)
- Page: https://freesound.org/people/8bitmyketison/sounds/699928/
- Download: https://cdn.freesound.org/previews/699/699928_15173053-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/lion_alarm_3.ogg` | 0.00-0.50 s |

## Freesound 516829: Lion Growl.wav

- Author: LilMati (freesound.org)
- Page: https://freesound.org/people/LilMati/sounds/516829/
- Download: https://cdn.freesound.org/previews/516/516829_6142149-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/lion_hurt_1.ogg` | 1.07-2.37 s |

## Freesound 69572: Lion roaring.mp3

- Author: Bidone (freesound.org)
- Page: https://freesound.org/people/Bidone/sounds/69572/
- Download: https://cdn.freesound.org/previews/69/69572_706955-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/lion_idle_1.ogg` | 19.83-20.93 s |
| `wild/lion_idle_2.ogg` | 21.43-22.43 s |
| `wild/lion_idle_3.ogg` | 22.97-24.02 s |
| `wild/lion_threat_1.ogg` | 13.58-15.23 s |
| `wild/lion_threat_2.ogg` | 16.24-17.54 s |

## Freesound 405211: Lions screaming during the night

- Author: felix.blume (freesound.org)
- Page: https://freesound.org/people/felix.blume/sounds/405211/
- Download: https://cdn.freesound.org/previews/405/405211_1661766-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/lion_threat_3.ogg` | 16.60-19.00 s |
| `wild/lion_hurt_3.ogg` | 0.34-1.22 s |
| `wild/lion_death_1.ogg` | 11.16-14.31 s |
| `wild/lion_death_2.ogg` | 5.80-7.10 s |

## Freesound 710300: Sheep baaing 5 - Norwegian sheep expressing itself concisely

- Author: michaelperfect (freesound.org)
- Page: https://freesound.org/people/michaelperfect/sounds/710300/
- Download: https://cdn.freesound.org/previews/710/710300_201532-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/sheep_alarm_2.ogg` | 0.02-0.92 s |
| `wild/sheep_death_2.ogg` | 0.02-1.82 s |

## Freesound 787563: Sheep - Lamb Bleat

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/787563/
- Download: https://cdn.freesound.org/previews/787/787563_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/sheep_hurt_1.ogg` | 0.03-0.83 s |

## Freesound 710299: Sheep baaing 4 - Norwegian sheep expressing itself concisely

- Author: michaelperfect (freesound.org)
- Page: https://freesound.org/people/michaelperfect/sounds/710299/
- Download: https://cdn.freesound.org/previews/710/710299_201532-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/sheep_hurt_2.ogg` | 0.07-0.87 s |
| `wild/sheep_death_1.ogg` | 0.07-1.92 s |

## Freesound 710296: Sheep baaing 1 - Norwegian sheep expressing itself concisely

- Author: michaelperfect (freesound.org)
- Page: https://freesound.org/people/michaelperfect/sounds/710296/
- Download: https://cdn.freesound.org/previews/710/710296_201532-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/sheep_idle_1.ogg` | 0.15-1.00 s |
| `wild/sheep_hurt_3.ogg` | 0.15-0.65 s |

## Freesound 710298: Sheep baaing 3 - Norwegian sheep expressing itself concisely

- Author: michaelperfect (freesound.org)
- Page: https://freesound.org/people/michaelperfect/sounds/710298/
- Download: https://cdn.freesound.org/previews/710/710298_201532-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/sheep_idle_2.ogg` | 0.05-1.30 s |
| `wild/sheep_alarm_3.ogg` | 0.05-0.75 s |

## Freesound 692900: Ewe Shetland Sheep Baa

- Author: satoristudios3 (freesound.org)
- Page: https://freesound.org/people/satoristudios3/sounds/692900/
- Download: https://cdn.freesound.org/previews/692/692900_14166079-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/sheep_idle_3.ogg` | 0.28-1.73 s |
| `wild/sheep_alarm_1.ogg` | 2.60-3.40 s |

## Freesound 395242: Wings flapping.mp3

- Author: DigPro120 (freesound.org)
- Page: https://freesound.org/people/DigPro120/sounds/395242/
- Download: https://cdn.freesound.org/previews/395/395242_7414526-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wings_1.ogg` | 0.05-1.45 s |
| `wild/wings_2.ogg` | 1.75-3.15 s |

## Freesound 528250: pigeons_fly away_ wing flaps_CsG

- Author: csaszi (freesound.org)
- Page: https://freesound.org/people/csaszi/sounds/528250/
- Download: https://cdn.freesound.org/previews/528/528250_1147184-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wings_3.ogg` | 0.10-1.05 s |
| `wild/wings_4.ogg` | 2.00-2.90 s |

## Freesound 153277: Beating wings of a pigeon

- Author: gerardcatala (freesound.org)
- Page: https://freesound.org/people/gerardcatala/sounds/153277/
- Download: https://cdn.freesound.org/previews/153/153277_2358240-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wings_5.ogg` | 0.00-1.20 s |

## Freesound 420450: Barking 1.wav

- Author: Mrthenoronha (freesound.org)
- Page: https://freesound.org/people/Mrthenoronha/sounds/420450/
- Download: https://cdn.freesound.org/previews/420/420450_2402876-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_alarm_1.ogg` | 0.00-0.30 s |

## Freesound 420449: Barking 2.wav

- Author: Mrthenoronha (freesound.org)
- Page: https://freesound.org/people/Mrthenoronha/sounds/420449/
- Download: https://cdn.freesound.org/previews/420/420449_2402876-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_alarm_2.ogg` | 0.00-0.30 s |

## Freesound 420448: Barking 3.wav

- Author: Mrthenoronha (freesound.org)
- Page: https://freesound.org/people/Mrthenoronha/sounds/420448/
- Download: https://cdn.freesound.org/previews/420/420448_2402876-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_alarm_3.ogg` | 0.00-0.30 s |

## Freesound 826930: Impatient Whiny Dog

- Author: qubodup (freesound.org)
- Page: https://freesound.org/people/qubodup/sounds/826930/
- Download: https://cdn.freesound.org/previews/826/826930_71257-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_death_1.ogg` | 0.05-1.60 s |
| `wild/wolf_death_2.ogg` | 8.18-9.73 s |
| `wild/wolf_death_3.ogg` | 4.10-5.10 s |

## Freesound 452180: bark yelp dog small int.flac

- Author: kyles (freesound.org)
- Page: https://freesound.org/people/kyles/sounds/452180/
- Download: https://cdn.freesound.org/previews/452/452180_612689-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_hurt_1.ogg` | 0.75-1.20 s |

## Freesound 160478: Dog's Yelping 7

- Author: unfa (freesound.org)
- Page: https://freesound.org/people/unfa/sounds/160478/
- Download: https://cdn.freesound.org/previews/160/160478_1038806-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_hurt_2.ogg` | 2.95-4.35 s |
| `wild/wolf_hurt_3.ogg` | 6.38-7.58 s |

## Freesound 220106: MonoWolves1.wav

- Author: JohnLaVine333 (freesound.org)
- Page: https://freesound.org/people/JohnLaVine333/sounds/220106/
- Download: https://cdn.freesound.org/previews/220/220106_450294-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_idle_1.ogg` | 2.50-6.60 s |

## Freesound 220104: MonoWolves3.wav

- Author: JohnLaVine333 (freesound.org)
- Page: https://freesound.org/people/JohnLaVine333/sounds/220104/
- Download: https://cdn.freesound.org/previews/220/220104_450294-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_idle_2.ogg` | 2.35-7.20 s |

## Freesound 753896: Adult and young wolves howling far away

- Author: Sacha.Julien (freesound.org)
- Page: https://freesound.org/people/Sacha.Julien/sounds/753896/
- Download: https://cdn.freesound.org/previews/753/753896_14889307-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_idle_3.ogg` | 0.00-3.30 s |

## Freesound 429123: 30-wolfGrowling.wav

- Author: cazadordoblekatana (freesound.org)
- Page: https://freesound.org/people/cazadordoblekatana/sounds/429123/
- Download: https://cdn.freesound.org/previews/429/429123_7312716-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_threat_1.ogg` | 0.25-0.90 s |

## Freesound 122183: Dog Growling Snarling Grumbling

- Author: qubodup (freesound.org)
- Page: https://freesound.org/people/qubodup/sounds/122183/
- Download: https://cdn.freesound.org/previews/122/122183_71257-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/wolf_threat_2.ogg` | 6.62-7.24 s |
| `wild/wolf_threat_3.ogg` | 7.78-8.73 s |

## Freesound 842289: Donkeys - Two Donkeys Braying, Close Perspective

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/842289/
- Download: https://cdn.freesound.org/previews/842/842289_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/zebra_alarm_3.ogg` | 17.20-17.80 s |
| `wild/zebra_hurt_1.ogg` | 12.48-12.76 s |
| `wild/zebra_hurt_2.ogg` | 13.10-13.35 s |
| `wild/zebra_hurt_3.ogg` | 18.44-19.64 s |
| `wild/zebra_death_1.ogg` | 13.60-17.05 s |

## Freesound 850661: Zebras - Grant's Zebra Braying

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/850661/
- Download: https://cdn.freesound.org/previews/850/850661_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/zebra_idle_1.ogg` | 0.08-3.28 s |
| `wild/zebra_alarm_1.ogg` | 0.08-1.23 s |
| `wild/zebra_alarm_2.ogg` | 1.20-3.20 s |

## Freesound 787564: Donkeys - Donkey Bray, Close Perspective

- Author: TheKingOfGeeks360 (freesound.org)
- Page: https://freesound.org/people/TheKingOfGeeks360/sounds/787564/
- Download: https://cdn.freesound.org/previews/787/787564_15895934-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `wild/zebra_idle_2.ogg` | 0.10-3.50 s |
| `wild/zebra_death_2.ogg` | 2.50-5.70 s |

## Freesound 792932: Slow Single Water Drop Splash

- Author: qubodup (freesound.org)
- Page: https://freesound.org/people/qubodup/sounds/792932/
- Download: https://cdn.freesound.org/previews/792/792932_71257-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `world/drip_1.ogg` | whole, trimmed |

## Freesound 546279: Single drip - dripping

- Author: Mega-X-stream (freesound.org)
- Page: https://freesound.org/people/Mega-X-stream/sounds/546279/
- Download: https://cdn.freesound.org/previews/546/546279_4937681-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `world/drip_2.ogg` | whole, trimmed |

## Freesound 842164: Water Drip Free

- Author: AardsReal (freesound.org)
- Page: https://freesound.org/people/AardsReal/sounds/842164/
- Download: https://cdn.freesound.org/previews/842/842164_13307919-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `world/drip_3.ogg` | whole, trimmed |

## Freesound 533146: Drop - Water

- Author: mattfinarelli (freesound.org)
- Page: https://freesound.org/people/mattfinarelli/sounds/533146/
- Download: https://cdn.freesound.org/previews/533/533146_7566729-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `world/drip_4.ogg` | whole, trimmed |

## Freesound 486272: R30-25a-Steady Heavy Rain.wav

- Author: craigsmith (freesound.org)
- Page: https://freesound.org/people/craigsmith/sounds/486272/
- Download: https://cdn.freesound.org/previews/486/486272_2524442-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `world/rain_1.ogg` | 14.0-17.2 s |
| `world/rain_2.ogg` | 30.0-33.2 s |
| `world/rain_3.ogg` | 38.0-41.2 s |

## Freesound 527499: Rain on Shed Roof

- Author: timothyd4y (freesound.org)
- Page: https://freesound.org/people/timothyd4y/sounds/527499/
- Download: https://cdn.freesound.org/previews/527/527499_288104-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `world/rain_roof_1.ogg` | 58.0-61.2 s, below 5 kHz |
| `world/rain_roof_2.ogg` | 50.0-53.2 s, below 5 kHz |
| `world/rain_roof_3.ogg` | 14.0-17.2 s, below 5 kHz |

## Freesound 352421: wind_gust.aif

- Author: joseph.larralde (freesound.org)
- Page: https://freesound.org/people/joseph.larralde/sounds/352421/
- Download: https://cdn.freesound.org/previews/352/352421_269279-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `world/wind_1.ogg` | 6.0-9.5 s |
| `world/wind_2.ogg` | 12.4-15.8 s |
| `world/wind_3.ogg` | 9.5-13.1 s |
| `world/wind_4.ogg` | 3.0-6.4 s |

## Freesound 117611: wind_howl2_stereo.wav

- Author: swiftoid (freesound.org)
- Page: https://freesound.org/people/swiftoid/sounds/117611/
- Download: https://cdn.freesound.org/previews/117/117611_854782-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `world/wind_howl_1.ogg` | 3.5-7.0 s |
| `world/wind_howl_2.ogg` | 7.0-10.5 s |
| `world/wind_howl_3.ogg` | 19.0-22.5 s |
| `world/wind_howl_4.ogg` | 25.0-28.5 s |

## Freesound 546759: Ambiance_Wind_Trees_Leaves_Moderate_Loop_Stereo.wav

- Author: Nox_Sound (freesound.org)
- Page: https://freesound.org/people/Nox_Sound/sounds/546759/
- Download: https://cdn.freesound.org/previews/546/546759_9250976-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `world/wind_leaves_1.ogg` | 3.0-6.0 s |
| `world/wind_leaves_2.ogg` | 6.5-9.5 s |
| `world/wind_leaves_3.ogg` | 12.0-15.0 s |

## Freesound 523389: Forest, close up of trees rustling in the wind.wav

- Author: Anya_Media (freesound.org)
- Page: https://freesound.org/people/Anya_Media/sounds/523389/
- Download: https://cdn.freesound.org/previews/523/523389_2010973-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `world/wind_leaves_4.ogg` | 60.0-63.0 s |


## Freesound 566040: Saw wood 3.wav

- Author: Pagey1969 (freesound.org)
- Page: https://freesound.org/people/Pagey1969/sounds/566040/
- Download: https://cdn.freesound.org/previews/566/566040_8615383-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/bench_1.ogg` | 3.05-3.77 s |
| `work/bench_2.ogg` | 8.13-8.85 s |


## Freesound 383725: Hand Saw Sawing Wood

- Author: deleted_user_7146007 (freesound.org)
- Page: https://freesound.org/people/deleted_user_7146007/sounds/383725/
- Download: https://cdn.freesound.org/previews/383/383725_7146007-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/bench_3.ogg` | 10.40-11.25 s |


## Freesound 379482: hoblovani dreva.wav (planing wood)

- Author: jandobes97 (freesound.org)
- Page: https://freesound.org/people/jandobes97/sounds/379482/
- Download: https://cdn.freesound.org/previews/379/379482_7029322-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/bench_4.ogg` | 4.28-5.23 s |
| `work/bench_5.ogg` | 6.48-7.43 s |


## Freesound 841053: chisel in the quarry

- Author: Artiom_Constantinov (freesound.org)
- Page: https://freesound.org/people/Artiom_Constantinov/sounds/841053/
- Download: https://cdn.freesound.org/previews/841/841053_7254491-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/mason_1.ogg` | 0.22-0.67 s |
| `work/mason_2.ogg` | 3.03-3.48 s |
| `work/mason_3.ogg` | 5.35-5.80 s |
| `work/mason_4.ogg` | 12.23-12.68 s |


## Freesound 718918: Stone or Brick Chisel Writing

- Author: black_trillium (freesound.org)
- Page: https://freesound.org/people/black_trillium/sounds/718918/
- Download: https://cdn.freesound.org/previews/718/718918_4889106-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/mason_5.ogg` | 7.88-8.38 s |


## Freesound 262251: ceramics_wheel.wav

- Author: vasifer (freesound.org)
- Page: https://freesound.org/people/vasifer/sounds/262251/
- Download: https://cdn.freesound.org/previews/262/262251_4673209-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/wheel_1.ogg` | 0.40-1.50 s |
| `work/wheel_2.ogg` | 2.60-3.70 s |
| `work/wheel_3.ogg` | 5.20-6.30 s |


## Freesound 319256: Mud_squishing.mp3

- Author: cormi (freesound.org)
- Page: https://freesound.org/people/cormi/sounds/319256/
- Download: https://cdn.freesound.org/previews/319/319256_1267745-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/wheel_4.ogg` | 6.00-6.90 s |
| `work/wheel_5.ogg` | 18.00-18.90 s |


## Freesound 625653: Heavy Scissors Cutting Leather

- Author: el_boss (freesound.org)
- Page: https://freesound.org/people/el_boss/sounds/625653/
- Download: https://cdn.freesound.org/previews/625/625653_9129912-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/leather_1.ogg` | 2.20-2.75 s |
| `work/leather_2.ogg` | 5.46-6.01 s |
| `work/leather_3.ogg` | 7.36-7.91 s |


## Freesound 628631: Heavy leather hole punch on vegetable tan leather

- Author: el_boss (freesound.org)
- Page: https://freesound.org/people/el_boss/sounds/628631/
- Download: https://cdn.freesound.org/previews/628/628631_9129912-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/leather_4.ogg` | 5.06-5.76 s |
| `work/leather_5.ogg` | 0.50-1.10 s |


## Freesound 434339: Hammer hitting an anvil

- Author: draftcraft (freesound.org)
- Page: https://freesound.org/people/draftcraft/sounds/434339/
- Download: https://cdn.freesound.org/previews/434/434339_6599148-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/anvil_1.ogg` | 4.11-4.96 s |
| `work/anvil_2.ogg` | 5.68-6.53 s |
| `work/anvil_3.ogg` | 2.33-3.18 s |
| `work/anvil_4.ogg` | 13.72-14.57 s |


## Freesound 849365: metallic sounds of a workbench

- Author: cazalrenoux (freesound.org)
- Page: https://freesound.org/people/cazalrenoux/sounds/849365/
- Download: https://cdn.freesound.org/previews/849/849365_291150-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `work/open_1.ogg` | 1.40-2.00 s |
| `work/open_2.ogg` | 12.70-13.40 s |


## Freesound 668606: stone_rock_mono_falling.wav

- Author: EricsSoundschmiede (freesound.org)
- Page: https://freesound.org/people/EricsSoundschmiede/sounds/668606/
- Download: https://cdn.freesound.org/previews/668/668606_1106446-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/stone_1.ogg` | 9.75-10.95 s |
| `crumble/stone_2.ogg` | 11.18-12.38 s |


## Freesound 655368: rock slide.wav

- Author: 21100495 (freesound.org)
- Page: https://freesound.org/people/21100495/sounds/655368/
- Download: https://cdn.freesound.org/previews/655/655368_13723333-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/stone_3.ogg` | 2.05-3.65 s |
| `crumble/stone_4.ogg` | 3.35-4.85 s |


## Freesound 232137: rock falling 010.wav

- Author: yottasounds (freesound.org)
- Page: https://freesound.org/people/yottasounds/sounds/232137/
- Download: https://cdn.freesound.org/previews/232/232137_3249786-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/stone_5.ogg` | 0.15-0.70 s |


## Freesound 567249: Bricks/Stones/Rocks/Gravel Falling

- Author: iwanPlays (freesound.org)
- Page: https://freesound.org/people/iwanPlays/sounds/567249/
- Download: https://cdn.freesound.org/previews/567/567249_7108319-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/gravel_1.ogg` | 0.00-1.40 s |
| `crumble/gravel_2.ogg` | 1.32-2.72 s |


## Freesound 179341: Gravel Impacts and Falls.wav

- Author: lolamadeus (freesound.org)
- Page: https://freesound.org/people/lolamadeus/sounds/179341/
- Download: https://cdn.freesound.org/previews/179/179341_544580-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/gravel_3.ogg` | 7.28-8.58 s |
| `crumble/gravel_4.ogg` | 11.30-12.50 s |


## Freesound 426174: Soil Slide

- Author: NeoSpica (freesound.org)
- Page: https://freesound.org/people/NeoSpica/sounds/426174/
- Download: https://cdn.freesound.org/previews/426/426174_7704891-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/dirt_1.ogg` | 1.18-2.58 s |
| `crumble/dirt_2.ogg` | 8.55-10.05 s |


## Freesound 348955: Crumble #8.wav

- Author: abstraktgeneriert (freesound.org)
- Page: https://freesound.org/people/abstraktgeneriert/sounds/348955/
- Download: https://cdn.freesound.org/previews/348/348955_3610778-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/dirt_3.ogg` | 0.10-1.20 s |


## Freesound 326304: sandfall5.wav

- Author: Wagna (freesound.org)
- Page: https://freesound.org/people/Wagna/sounds/326304/
- Download: https://cdn.freesound.org/previews/326/326304_3271346-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/sand_1.ogg` | 0.65-2.05 s |
| `crumble/sand_2.ogg` | 2.45-3.85 s |


## Freesound 627070: SAND POUR.wav

- Author: nicoproson (freesound.org)
- Page: https://freesound.org/people/nicoproson/sounds/627070/
- Download: https://cdn.freesound.org/previews/627/627070_457982-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/sand_3.ogg` | 13.12-14.42 s |
| `crumble/sand_4.ogg` | 28.90-30.30 s |


## Freesound 669457: SFX_wood_cracking.wav

- Author: EricsSoundschmiede (freesound.org)
- Page: https://freesound.org/people/EricsSoundschmiede/sounds/669457/
- Download: https://cdn.freesound.org/previews/669/669457_1106446-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/wood_1.ogg` | 0.70-1.70 s |
| `crumble/wood_2.ogg` | 1.92-3.02 s |


## Freesound 670300: tree cut down.wav

- Author: bruno.auzet (freesound.org)
- Page: https://freesound.org/people/bruno.auzet/sounds/670300/
- Download: https://cdn.freesound.org/previews/670/670300_11519060-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `crumble/wood_3.ogg` | 4.72-6.02 s |
| `crumble/wood_4.ogg` | 6.05-7.15 s |

## Freesound 438845: G54-26-Person Swimming.wav

- Author: craigsmith (freesound.org)
- Page: https://freesound.org/people/craigsmith/sounds/438845/
- Download: https://cdn.freesound.org/previews/438/438845_2524442-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `player/swim_1.ogg` | 28.95-29.63 s |
| `player/swim_2.ogg` | 3.75-4.57 s |
| `player/swim_3.ogg` | 8.87-9.65 s |
| `player/swim_4.ogg` | 15.28-15.86 s |
| `player/swim_5.ogg` | 18.94-19.53 s |

## Freesound 530158: POOL SWIMMING R-L

- Author: tbsounddesigns (freesound.org)
- Page: https://freesound.org/people/tbsounddesigns/sounds/530158/
- Download: https://cdn.freesound.org/previews/530/530158_5387364-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `player/swim_6.ogg` | 4.34-4.96 s |

## Freesound 342932: Wading in Shallow Water.wav

- Author: ryansitz (freesound.org)
- Page: https://freesound.org/people/ryansitz/sounds/342932/
- Download: https://cdn.freesound.org/previews/342/342932_5800259-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `player/wade_1.ogg` | 0.21-1.01 s |
| `player/wade_2.ogg` | 14.47-15.24 s |
| `player/wade_3.ogg` | 7.31-8.15 s |
| `player/wade_4.ogg` | 9.13-10.05 s |
| `player/wade_5.ogg` | 10.35-11.19 s |
| `player/wade_6.ogg` | 18.86-19.40 s |

## Freesound 585744: Foley_Natural_Water_Jump_Mono.wav

- Author: Nox_Sound (freesound.org)
- Page: https://freesound.org/people/Nox_Sound/sounds/585744/
- Download: https://cdn.freesound.org/previews/585/585744_9250976-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `player/splash_1.ogg` | 0.03-1.60 s |
| `player/splash_2.ogg` | 5.19-6.70 s |
| `player/splash_3.ogg` | 9.64-11.15 s |

## The stakes (`player/stake_*.ogg`)

Running onto a bundle of sharpened poles is not a knife: it is wood
knocked and shoved, a twig giving, cloth catching, and a body stopping.
No single CC0 recording is that, so each of the five is two or three of
the recordings below laid over each other at the offsets given (seconds
into the clip) -- the foley way, every layer a real thing disturbed and
nothing generated. Summed, high-passed at 70 Hz, levelled by energy to
-19 dB with the peak held under -1 dB, faded out over the last 100 ms.
They replace a stake "into a vampire" and knives into melons, which were
a horror film's stab and which the player called bad.

| file (in `player/`) | layers |
|---|---|
| `stake_1.ogg` | 461697 0.14-0.66 s |
| `stake_2.ogg` | 584894 4.04-4.40 s; 504626 0.36-0.82 s at +0.02 s, -3 dB |
| `stake_3.ogg` | 584894 4.61-4.95 s; 853591 0.97-1.45 s at +0.03 s, -2 dB |
| `stake_4.ogg` | 452109 0.46-0.70 s, -9 dB; 853591 10.75-11.23 s at +0.02 s |
| `stake_5.ogg` | 584894 1.87-2.25 s; 415202 1.13-1.30 s at +0.05 s, -8 dB; 504626 0.36-0.80 s at +0.06 s, -4 dB |

## Freesound 461697: Body falls into debris

- Author: leonelmail (freesound.org)
- Page: https://freesound.org/people/leonelmail/sounds/461697/
- Download: https://cdn.freesound.org/previews/461/461697_4437257-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `player/stake_1.ogg` | 0.14-0.66 s |

## Freesound 584894: throwing logs on dirt.wav

- Author: KrystianPawlowski (freesound.org)
- Page: https://freesound.org/people/KrystianPawlowski/sounds/584894/
- Download: https://cdn.freesound.org/previews/584/584894_13194852-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `player/stake_2.ogg` | 4.04-4.40 s |
| `player/stake_3.ogg` | 4.61-4.95 s |
| `player/stake_5.ogg` | 1.87-2.25 s |

## Freesound 504626: BODY FALL - V HVY - DIRT

- Author: leonelmail (freesound.org)
- Page: https://freesound.org/people/leonelmail/sounds/504626/
- Download: https://cdn.freesound.org/previews/504/504626_4437257-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `player/stake_2.ogg` | 0.36-0.82 s |
| `player/stake_5.ogg` | 0.36-0.80 s |

## Freesound 853591: A body falling to the ground, with leaf crush

- Author: Wigglesworth (freesound.org)
- Page: https://freesound.org/people/Wigglesworth/sounds/853591/
- Download: https://cdn.freesound.org/previews/853/853591_6775466-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `player/stake_3.ogg` | 0.97-1.45 s |
| `player/stake_4.ogg` | 10.75-11.23 s |

## Freesound 452109: branch break snap crunch nice.wav

- Author: kyles (freesound.org)
- Page: https://freesound.org/people/kyles/sounds/452109/
- Download: https://cdn.freesound.org/previews/452/452109_612689-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `player/stake_4.ogg` | 0.46-0.70 s |

## Freesound 415202: Cloth Tearing.mp3

- Author: Yin_Yang_Jake007 (freesound.org)
- Page: https://freesound.org/people/Yin_Yang_Jake007/sounds/415202/
- Download: https://cdn.freesound.org/previews/415/415202_7919598-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)

| file | cut from |
|---|---|
| `player/stake_5.ogg` | 1.13-1.30 s |

## Freesound 437111: G38-16-Four Horse Snorts.wav

- Author: craigsmith (freesound.org)
- Page: https://freesound.org/people/craigsmith/sounds/437111/
- Download: https://cdn.freesound.org/previews/437/437111_2524442-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: four snorts of one horse, close; cut into the calm horse's blows (high-passed at 120 Hz, 16 kHz)

| file | cut from |
|---|---|
| `wild/horse_idle_1.ogg` | 0.45-1.25 s |
| `wild/horse_idle_2.ogg` | 2.00-2.50 s |
| `wild/horse_idle_3.ogg` | 4.30-4.85 s |
| `wild/horse_idle_4.ogg` | 6.12-6.75 s |

## Freesound 437110: G38-15-Perfect Horse Whinny.wav

- Author: craigsmith (freesound.org)
- Page: https://freesound.org/people/craigsmith/sounds/437110/
- Download: https://cdn.freesound.org/previews/437/437110_2524442-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: one full whinny, close; the whole of it is an alarm, its first 0.7 s a wound

| file | cut from |
|---|---|
| `wild/horse_alarm_1.ogg` | 10.55-12.60 s |
| `wild/horse_hurt_3.ogg` | 10.55-11.25 s |

## Freesound 347036: horse's whinny

- Author: Kubuzz (freesound.org)
- Page: https://freesound.org/people/Kubuzz/sounds/347036/
- Download: https://cdn.freesound.org/previews/347/347036_1708499-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: a horse's whinny; the whole of it is an alarm, its first 0.7 s a wound

| file | cut from |
|---|---|
| `wild/horse_alarm_2.ogg` | 1.25-3.30 s |
| `wild/horse_hurt_1.ogg` | 1.25-1.95 s |

## Freesound 149024: Horse_Whinny.wav

- Author: foxen10 (freesound.org)
- Page: https://freesound.org/people/foxen10/sounds/149024/
- Download: https://cdn.freesound.org/previews/149/149024_2581089-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: a whinny and the short blasts round it; the whinny is an alarm, the loud blast before it a wound

| file | cut from |
|---|---|
| `wild/horse_alarm_3.ogg` | 3.45-5.25 s |
| `wild/horse_hurt_2.ogg` | 2.25-2.70 s |

## Freesound 479705: R13-33-Horse Breath and Snort.wav

- Author: craigsmith (freesound.org)
- Page: https://freesound.org/people/craigsmith/sounds/479705/
- Download: https://cdn.freesound.org/previews/479/479705_2524442-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: a horse breathing out and snorting; the long breath and the last snort are its death

| file | cut from |
|---|---|
| `wild/horse_death_1.ogg` | 8.00-9.70 s |
| `wild/horse_death_2.ogg` | 20.15-21.00 s |

## Freesound 481917: R26-18-Foley Horse Hooves Walking.wav

- Author: craigsmith (freesound.org)
- Page: https://freesound.org/people/craigsmith/sounds/481917/
- Download: https://cdn.freesound.org/previews/481/481917_2524442-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: hooves walking on soft ground, recorded close as foley; pieces of the walk, levelled by energy

| file | cut from |
|---|---|
| `step/hoof_walk_1.ogg` | 16.40-17.40 s |
| `step/hoof_walk_2.ogg` | 19.20-20.20 s |
| `step/hoof_walk_3.ogg` | 23.60-24.60 s |
| `step/hoof_walk_4.ogg` | 31.20-32.20 s |

## Freesound 675422: S01-01_Horse trots in on hard dirt; stops; trotting out.wav

- Author: craigsmith (freesound.org)
- Page: https://freesound.org/people/craigsmith/sounds/675422/
- Download: https://cdn.freesound.org/previews/675/675422_2524442-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: a horse trotting in on hard dirt; pieces of the trot

| file | cut from |
|---|---|
| `step/hoof_trot_1.ogg` | 5.25-6.00 s |
| `step/hoof_trot_2.ogg` | 7.10-7.85 s |
| `step/hoof_trot_3.ogg` | 9.00-9.75 s |
| `step/hoof_trot_4.ogg` | 13.80-14.55 s |

## Freesound 368583: horse_galloping.wav

- Author: telezon (freesound.org)
- Page: https://freesound.org/people/telezon/sounds/368583/
- Download: https://cdn.freesound.org/previews/368/368583_5492735-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: a horse galloping; pieces two strides long

| file | cut from |
|---|---|
| `step/hoof_gallop_1.ogg` | 3.55-4.51 s |
| `step/hoof_gallop_2.ogg` | 11.90-12.86 s |

## Freesound 175356: Horse Galloping.wav

- Author: Max_Headroom (freesound.org)
- Page: https://freesound.org/people/Max_Headroom/sounds/175356/
- Download: https://cdn.freesound.org/previews/175/175356_2861652-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: a horse galloping; pieces two strides long

| file | cut from |
|---|---|
| `step/hoof_gallop_3.ogg` | 2.10-3.06 s |
| `step/hoof_gallop_4.ogg` | 3.20-4.16 s |

## Freesound 182504: Horse Clip Clopping Downhill (stereo)

- Author: swiftoid (freesound.org)
- Page: https://freesound.org/people/swiftoid/sounds/182504/
- Download: https://cdn.freesound.org/previews/182/182504_854782-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: a horse clip-clopping downhill on a hard road; pieces of a walk on stone

| file | cut from |
|---|---|
| `step/hoof_walk_hard_1.ogg` | 6.40-7.40 s |
| `step/hoof_walk_hard_2.ogg` | 9.80-10.80 s |

## Freesound 479679: R13-07-Horse on Wood.wav

- Author: craigsmith (freesound.org)
- Page: https://freesound.org/people/craigsmith/sounds/479679/
- Download: https://cdn.freesound.org/previews/479/479679_2524442-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: a horse walking on wooden boards; pieces of a walk on wood

| file | cut from |
|---|---|
| `step/hoof_walk_hard_3.ogg` | 5.60-6.60 s |
| `step/hoof_walk_hard_4.ogg` | 9.35-10.35 s |

## Freesound 549882: Horses Pavement Then Cobblestone.m4a

- Author: guynoland (freesound.org)
- Page: https://freesound.org/people/guynoland/sounds/549882/
- Download: https://cdn.freesound.org/previews/549/549882_8234803-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: horses on pavement and then cobblestones; pieces of the trot on stone

| file | cut from |
|---|---|
| `step/hoof_trot_hard_1.ogg` | 15.10-15.85 s |
| `step/hoof_trot_hard_2.ogg` | 18.00-18.75 s |
| `step/hoof_trot_hard_3.ogg` | 20.20-20.95 s |
| `step/hoof_trot_hard_4.ogg` | 31.30-32.05 s |

## Freesound 637429: crickets field closeup high frequency.flac

- Author: kyles (freesound.org)
- Page: https://freesound.org/people/kyles/sounds/637429/
- Download: https://cdn.freesound.org/previews/637/637429_612689-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: crickets in a field, close, high frequency; high-passed at 2.5 kHz, cut as a bed (3.2 s, equal-power fades of 0.8 s)

| file | cut from |
|---|---|
| `world/crickets_1.ogg` | 2.00-5.20 s |
| `world/crickets_2.ogg` | 30.00-33.20 s |

## Freesound 265539: Night crickets @ Almargem.wav

- Author: Refrain (freesound.org)
- Page: https://freesound.org/people/Refrain/sounds/265539/
- Download: https://cdn.freesound.org/previews/265/265539_4058337-hq.mp3 (the page's high-quality preview; the original needs an account, and none was used)
- Licence: CC0 1.0 (the page says "Creative Commons 0" and links creativecommons.org/publicdomain/zero/1.0; checked when it was downloaded)
- What it is: night crickets at Almargem, Portugal; high-passed at 2.5 kHz, cut as a bed (3.2 s, equal-power fades of 0.8 s)

| file | cut from |
|---|---|
| `world/crickets_3.ogg` | 3.00-6.20 s |
| `world/crickets_4.ogg` | 60.00-63.20 s |
