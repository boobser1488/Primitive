#!/usr/bin/env python3
"""Draws the five animals added for the woods and the fresh water, and the
two pictures that came with them: assets/textures/animals/{fox,elk,beaver,
duck,frog}.png, food/frog.png and terrain/beaver_dam.png.

**Recoloured, not redrawn**, which is the house rule for a new animal
(`engine::texture::ANIMAL_SHEETS`): what makes an animal here look as if it
belongs is the grain of its coat, and every sheet already in the game
carries that grain. So each tile is taken from the nearest cousin and has
its colour replaced while its light and shade are kept -- a pixel that was a
fifth brighter than its tile's average stays a fifth brighter than the new
colour. The eyes are kept exactly: they are the darkest pixels of a head
tile, and a recoloured eye is a smudge.

What is drawn rather than recoloured is only what no cousin has: the white
tip of a fox's brush, an elk's palm, the scales of a beaver's tail and the
orange of its teeth, a mallard's collar and the blue of its wing, a frog's
spots and its golden eye.

Run from anywhere; it writes beside the other pictures. Deterministic: the
same run twice writes the same bytes.
"""

import io
import os
import random

import numpy as np
from PIL import Image

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), os.pardir, "assets", "textures")
T = 16

# The slot order of `animal_model::Skin::slot`: four across, three down.
HIDE, HEAD, HEAD_M, FACE, SNOUT, NOSE, EAR, HOOF, FUR, TUSK, ANTLER, HORN = range(12)


def sheet(name):
    return np.array(Image.open(os.path.join(ROOT, "animals", name + ".png")).convert("RGBA"), dtype=np.float32)


def tile(src, slot):
    x, y = (slot % 4) * T, (slot // 4) * T
    return src[y:y + T, x:x + T].copy()


def lum(t):
    return 0.3 * t[..., 0] + 0.59 * t[..., 1] + 0.11 * t[..., 2]


def colour(t, rgb, contrast=1.0, keep_dark=False):
    """The tile's light and shade over a new colour. `keep_dark` leaves the
    darkest pixels alone: the eyes, and the line of a mouth."""
    out = t.copy()
    l = lum(t)
    solid = t[..., 3] > 0
    if not solid.any():
        return out
    mean = l[solid].mean()
    ratio = 1.0 + contrast * (l / max(mean, 1.0) - 1.0)
    for c in range(3):
        out[..., c] = np.clip(rgb[c] * ratio, 0, 255)
    if keep_dark:
        eye = (l < 45) & solid
        out[eye] = t[eye]
    out[..., 3] = np.where(solid, 255, 0)
    return out


def solid_like(t):
    """A tile's grain, made fully opaque -- for a slot the cousin left
    blank."""
    out = t.copy()
    out[..., 3] = 255
    return out


def put(t, x, y, rgb, w=1, h=1):
    t[y:y + h, x:x + w, :3] = rgb
    t[y:y + h, x:x + w, 3] = 255


def blank():
    return np.zeros((T, T, 4), dtype=np.float32)


def write_sheet(name, tiles):
    out = np.zeros((3 * T, 4 * T, 4), dtype=np.float32)
    for slot, t in tiles.items():
        x, y = (slot % 4) * T, (slot // 4) * T
        out[y:y + T, x:x + T] = t
    save(Image.fromarray(np.round(out).astype(np.uint8), "RGBA"), os.path.join("animals", name + ".png"))


def save(img, rel):
    """Written at full effort: see `embedded::no_picture_is_shipped_in_a_lazily_written_png`."""
    buf = io.BytesIO()
    img.save(buf, "PNG", optimize=True, compress_level=9)
    with open(os.path.join(ROOT, rel), "wb") as f:
        f.write(buf.getvalue())


def fox():
    wolf = sheet("wolf")
    rust, cream, black = (198, 100, 44), (232, 222, 204), (46, 34, 30)
    head = colour(tile(wolf, HEAD), rust, keep_dark=True)
    # White cheeks and throat: the lower third of the head, which is what
    # makes a fox's face read from across a meadow.
    head[11:, :, :3] = colour(tile(wolf, HEAD), cream, 0.5)[11:, :, :3]
    face = colour(tile(wolf, FACE), rust, 0.8)
    face[10:, :, :3] = colour(tile(wolf, FACE), cream, 0.5)[10:, :, :3]
    nose = colour(tile(wolf, NOSE), cream, 0.5)
    put(nose, 5, 1, black, 6, 4)
    fur = colour(tile(wolf, FUR), (186, 92, 40), 1.2)
    write_sheet("fox", {
        HIDE: colour(tile(wolf, HIDE), rust, 1.1),
        HEAD: head,
        HEAD_M: head[:, ::-1].copy(),
        FACE: face,
        SNOUT: colour(tile(wolf, SNOUT), cream, 0.6),
        NOSE: nose,
        EAR: colour(tile(wolf, EAR), black, 0.8),
        HOOF: colour(tile(wolf, HOOF), black, 0.9),
        # The white tip of the brush wears the muzzle's cream (`SNOUT`): the
        # horn slot is the antelope's (`only_the_antelope_wears_horns`).
        FUR: fur,
    })


def elk():
    deer = sheet("deer")
    dark, legs = (70, 54, 42), (156, 138, 114)
    head = colour(tile(deer, HEAD), dark, 1.1, keep_dark=True)
    nose = colour(tile(deer, NOSE), (78, 60, 48), 0.8)
    # Nostrils: two dark slits low on the front of the muzzle.
    put(nose, 3, 9, (28, 22, 20), 3, 2)
    put(nose, 10, 9, (28, 22, 20), 3, 2)
    antler = colour(tile(deer, ANTLER), (210, 194, 160), 1.3)
    # **A palm**: a band of pale bone across the tile with notched tines on
    # its top edge, where a deer's antler tile is a single beam.
    rng = random.Random(21)
    for x in range(T):
        for y in range(T):
            shade = 196 + rng.randint(-14, 10) - (y // 4) * 6
            put(antler, x, y, (shade, shade - 16, shade - 46))
        if x % 4 in (0, 1):
            put(antler, x, 0, (236, 224, 196), 1, 2)
    write_sheet("elk", {
        HIDE: colour(tile(deer, HIDE), dark, 1.2),
        HEAD: head,
        HEAD_M: head[:, ::-1].copy(),
        FACE: colour(tile(deer, FACE), dark, 1.0),
        SNOUT: colour(tile(deer, SNOUT), (84, 64, 50), 1.0),
        NOSE: nose,
        EAR: colour(tile(deer, EAR), (60, 46, 38), 0.8),
        # Pale stockings: an elk's legs are the lightest part of it.
        HOOF: colour(tile(deer, HOOF), legs, 0.9),
        FUR: colour(solid_like(tile(deer, HIDE)), (54, 42, 34), 1.2),
        ANTLER: antler,
    })


def beaver():
    rat = sheet("rat")
    chestnut = (112, 72, 44)
    head = colour(tile(rat, HEAD), chestnut, 1.0, keep_dark=True)
    # The front of the head from the hide's grain, not the head's: the head
    # tile has the eyes on it, and a front face wearing them is a face with
    # four eyes.
    face = colour(tile(rat, HIDE), chestnut, 0.8)
    # The teeth: two orange incisors at the bottom of the front of the head.
    put(face, 6, 12, (222, 126, 42), 2, 4)
    put(face, 8, 12, (206, 112, 36), 2, 4)
    nose = colour(tile(rat, HIDE), (92, 62, 42), 0.8)
    put(nose, 5, 2, (34, 26, 24), 6, 3)
    tail = blank()
    # **The paddle**: black, scaled in a diamond lattice.
    for y in range(T):
        for x in range(T):
            line = (x + y) % 4 == 0 or (x - y) % 4 == 0
            base = 74 if line else 36
            put(tail, x, y, (base, base - 2, base + 2))
    write_sheet("beaver", {
        HIDE: colour(tile(rat, HIDE), chestnut, 1.2),
        HEAD: head,
        HEAD_M: head[:, ::-1].copy(),
        FACE: face,
        SNOUT: colour(tile(rat, HIDE), (96, 64, 44), 0.8),
        NOSE: nose,
        EAR: colour(tile(rat, HIDE), (72, 48, 34), 0.8),
        HOOF: colour(solid_like(tile(rat, HIDE)), (46, 38, 34), 0.6),
        # In the fur's square: a tail wearing the tusk's would be a tusk to
        # the skeleton (`skeleton_parts`), which keeps what wears ivory.
        FUR: tail,
    })


def duck():
    fowl, gull = sheet("fowl"), sheet("gull")
    green, grey, bill = (40, 96, 58), (150, 146, 140), (196, 184, 72)
    head = colour(tile(gull, HEAD), green, 1.0, keep_dark=True)
    neck = colour(solid_like(tile(gull, HIDE)), green, 1.0)
    # The white collar at the foot of the neck: the mallard's.
    put(neck, 0, 13, (238, 238, 232), T, 2)
    nose = colour(solid_like(tile(gull, HIDE)), bill, 0.6)
    put(nose, 6, 1, (40, 36, 30), 4, 3)
    wing = colour(solid_like(tile(fowl, HIDE)), (122, 118, 114), 0.9)
    # The speculum: a band of blue edged in white across the wing.
    put(wing, 0, 9, (240, 240, 236), T, 1)
    put(wing, 0, 10, (58, 74, 160), T, 3)
    put(wing, 0, 13, (240, 240, 236), T, 1)
    write_sheet("duck", {
        HIDE: colour(tile(fowl, HIDE), grey, 0.7),
        HEAD: head,
        HEAD_M: head[:, ::-1].copy(),
        FACE: colour(solid_like(tile(gull, HIDE)), green, 1.0),
        SNOUT: colour(solid_like(tile(gull, HIDE)), bill, 0.6),
        NOSE: nose,
        EAR: neck,
        HOOF: colour(solid_like(tile(gull, HIDE)), (228, 124, 42), 0.6),
        FUR: wing,
    })


def frog():
    hare = sheet("hare")
    green, dark, pale = (88, 124, 54), (44, 64, 30), (204, 198, 142)
    rng = random.Random(83)

    def spotted(t, rgb):
        out = colour(solid_like(t), rgb, 1.2)
        for _ in range(5):
            x, y = rng.randint(0, T - 3), rng.randint(0, T - 3)
            put(out, x, y, dark, 2 + rng.randint(0, 1), 2)
        return out

    head = colour(solid_like(tile(hare, HIDE)), green, 1.0)
    # The mouth: a dark line along the bottom of the side of the head.
    put(head, 0, 12, (38, 44, 28), T, 1)
    face = colour(solid_like(tile(hare, HIDE)), green, 1.0)
    face[9:, :, :3] = colour(solid_like(tile(hare, HIDE)), pale, 0.4)[9:, :, :3]
    eye = blank()
    # **The eye on top of the head**, gold with a black slot: the one part of
    # a frog that shows over the water.
    for y in range(T):
        for x in range(T):
            put(eye, x, y, (196, 156, 48) if (x + y) % 5 else (220, 182, 70))
    put(eye, 3, 6, (16, 14, 12), 10, 4)
    nostrils = colour(solid_like(tile(hare, HIDE)), (120, 150, 72), 0.8)
    put(nostrils, 4, 5, (34, 40, 24), 2, 2)
    put(nostrils, 10, 5, (34, 40, 24), 2, 2)
    legs = spotted(tile(hare, HIDE), (80, 116, 50))
    for y in (3, 8, 13):
        put(legs, 0, y, dark, T, 1)
    write_sheet("frog", {
        HIDE: spotted(tile(hare, HIDE), green),
        HEAD: head,
        HEAD_M: head[:, ::-1].copy(),
        FACE: face,
        SNOUT: colour(solid_like(tile(hare, HIDE)), (120, 150, 72), 0.8),
        NOSE: nostrils,
        EAR: eye,
        HOOF: colour(solid_like(tile(hare, HIDE)), (110, 146, 66), 0.8),
        FUR: legs,
    })


def frog_item():
    """The frog in the pack: a green frog seen from above, on a clear square."""
    t = blank()
    green, dark, pale = (92, 130, 56), (46, 66, 30), (200, 196, 140)
    body = [(5, 5, 6, 7), (6, 3, 4, 2)]
    for x, y, w, h in body:
        put(t, x, y, green, w, h)
    for x, y in ((4, 11), (11, 11), (3, 12), (12, 12), (2, 13), (13, 13)):
        put(t, x, y, green)
    for x, y in ((4, 6), (11, 6), (3, 5), (12, 5)):
        put(t, x, y, green)
    put(t, 6, 3, (206, 164, 52))
    put(t, 9, 3, (206, 164, 52))
    for x, y in ((7, 7), (8, 9), (6, 10)):
        put(t, x, y, dark)
    put(t, 7, 12, pale, 2, 1)
    save(Image.fromarray(np.round(t).astype(np.uint8), "RGBA"), os.path.join("food", "frog.png"))


def dam():
    """A beaver's dam: sticks laid every way through a packing of wet mud."""
    rng = random.Random(1784)
    t = blank()
    for y in range(T):
        for x in range(T):
            m = 70 + rng.randint(-8, 8)
            put(t, x, y, (m, m - 12, m - 26))
    for _ in range(9):
        x, y = rng.randint(0, T - 1), rng.randint(0, T - 1)
        dx, dy = rng.choice(((1, 0), (1, 1), (1, -1), (0, 1)))
        length = rng.randint(5, 11)
        shade = rng.randint(118, 150)
        for i in range(length):
            px, py = (x + dx * i) % T, (y + dy * i) % T
            put(t, px, py, (shade, shade - 36, shade - 76))
            if i % 3 == 0:
                put(t, px, (py + 1) % T, (shade - 40, shade - 66, shade - 90))
    save(Image.fromarray(np.round(t).astype(np.uint8), "RGBA"), os.path.join("terrain", "beaver_dam.png"))


if __name__ == "__main__":
    for draw in (fox, elk, beaver, duck, frog, frog_item, dam):
        draw()
