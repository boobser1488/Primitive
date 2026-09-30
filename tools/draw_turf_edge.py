#!/usr/bin/env python3
"""Draws the cut edge of turf: assets/textures/terrain/*_edge.png.

What the riser of a turf step wears -- the face between one lip of a
hillside and the next one down (`ground::turf_wraps_the_side`). It was the
top's own picture, so every step of a meadow was green on the same green
and a hillside of them read as mush ("сливаются в кашу"); before that it
was the side's picture, soil under a fringe, and a hillside read as brown
stripes. This is the thing between: the edge of a mat of grass, blades
hanging over it, the thatch and the roots in the shade under them, and the
soil only where the step is tall enough to show it.

**A recolouring, not a new drawing** (the pack's rule): every colour here
is one of the top's or the side's own, so the climate tint -- which the
shader weighs by how green a texel is -- lands on it exactly as it lands on
the turf round it, and the soil is the soil the side already had.

**The rows are the design.** A side face of a part-height block wears the
top rows of its picture, as many as it is tall (`mesh::side_crop`), so a
quarter step shows rows 0..3, a half step 0..7 and a whole block all
sixteen. Rows 0..3 are therefore the picture of four risers in five: blades
standing in vertical strokes, lit at their tips and darker between, which
is a different grain from the top's speckle at every distance. The dark
line of thatch comes next, where a half step ends -- the contact shadow a
step casts at its own foot -- and the soil comes last, for the steep banks
where a slope is a bank.

Run from the repository root:  python tools/draw_turf_edge.py
"""

import os
import random
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
TERRAIN = os.path.join(HERE, os.pardir, "assets", "textures", "terrain")
N = 16


def luma(c):
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]


def palette(picture, rows, keep):
    """The distinct colours of some rows of a picture, dark to light, that
    `keep` accepts."""
    seen = set()
    for y in rows:
        for x in range(N):
            c = picture.getpixel((x, y))[:3]
            if keep(c):
                seen.add(c)
    return sorted(seen, key=luma)


def draw(top_name, side_name, out_name, is_blade):
    top = Image.open(os.path.join(TERRAIN, top_name)).convert("RGBA")
    side = Image.open(os.path.join(TERRAIN, side_name)).convert("RGBA")
    blades = palette(top, range(N), lambda c: True)
    soil = palette(side, range(6, N), lambda c: not is_blade(c))
    # A fixed seed per picture: the file is an asset, and running this
    # again must write the same bytes.
    rng = random.Random(out_name)
    out = Image.new("RGBA", (N, N))
    # How far each column's blades hang: most end between rows 6 and 9 --
    # past the first half, so a quarter step is grass from edge to edge and
    # a whole block still two thirds green -- and a few strands reach on down.
    reach = [rng.choice([6, 7, 7, 8, 8, 9]) for _ in range(N)]
    for x in rng.sample(range(N), 3):
        reach[x] = rng.choice([11, 12, 13])
    for x in range(N):
        # Blades are two texels wide at most; a lone texel between two of
        # them is the dark gap, and it shifts every few columns.
        gap = (x % 3 == 2) != (x >= 8)
        for y in range(N):
            if y < reach[x]:
                # Lit at the tips and darker toward the root: the blade's own
                # length in the palette, from its lighter end down.
                depth = y / max(reach[x], 1)
                index = int(round((len(blades) - 1) * (0.95 - 0.8 * depth)))
                if gap:
                    index -= 4
                index += rng.choice([-1, 0, 0, 1])
                colour = blades[max(0, min(len(blades) - 1, index))]
            elif y < reach[x] + 2:
                # The thatch: dead blades and the first roots, in the shade
                # of the living ones. The darkest soil there is.
                colour = soil[rng.choice([0, 0, 1])]
            else:
                colour = side.getpixel((x, y))[:3]
                # Shaded a little under the overhang, less further down.
                shade = 0.82 + 0.18 * min(1.0, (y - reach[x] - 2) / 5)
                colour = min(soil, key=lambda c: abs(luma(c) - luma(colour) * shade))
                # A root, now and then, in the soil's lightest colour.
                if rng.random() < 0.07:
                    colour = soil[-1]
            out.putpixel((x, y), colour + (255,))
    out.save(os.path.join(TERRAIN, out_name))
    print("wrote", out_name)


def green(c):
    return c[1] > c[0] + 20 and c[1] > c[2] + 20


def straw(c):
    return c[0] > 120


draw("grass_top.png", "grass_side.png", "grass_edge.png", green)
draw("dry_turf_top.png", "dry_turf_side.png", "dry_turf_edge.png", straw)
