#!/usr/bin/env python3
"""Draws the interface's own pictures: assets/textures/ui/*.png.

Every one of these is a *multiplier*, not a colour. The shader does
sampled.rgb * tint.rgb, the tint is the theme's surface colour, and a
texel of 1.0 leaves that colour exactly where it was. So the picture
carries the light, the shade and the stitching; the theme carries the
material. One skin then serves the stone screens and the menu's dark
one, which is what the game already promises ("one interface in two
lights"), and a picture that had its own colour would break that
promise the moment the menu opened.

Mid grey is 1.0 and the range runs to 2.5, because the tint is
multiplied by SKIN_GAIN = 2.5 on its way in (see widgets::SKIN_GAIN):
a byte of 255 can only ever darken a colour, and a bevel needs a
highlight as much as it needs a shadow. Two and a half is not a round
number picked for looks -- it is what the bevel this skin replaces
already was, `Theme::STONE.light / .panel` = 2.11, with a little room
over it.
"""

import math
import os
import random
import io
import time
from PIL import Image

N = 32
GAIN = 2.5
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), os.pardir, "assets", "textures", "ui")

# One texel of the skin is one font pixel on the screen (widgets::PIXEL),
# so the border, the stitching and the letters are all drawn at the same
# size. The numbers below are in texels.


def enc(m):
    """A multiplier 0..2 as the sRGB byte the array stores.

    The array is Rgba8UnormSrgb, so the card undoes the transfer curve
    before the shader ever sees the value: what has to be linear is the
    multiplier, not the byte. Encoding here rather than writing m*127
    straight out is the difference between a bevel that is one stop
    brighter and one that is barely visible.
    """
    l = max(0.0, min(1.0, m / GAIN))
    s = 12.92 * l if l <= 0.0031308 else 1.055 * (l ** (1 / 2.4)) - 0.055
    return int(round(s * 255))


def blank(v=1.0):
    return [[v] * N for _ in range(N)]


def save(m, a, name):
    im = Image.new("RGBA", (N, N))
    for y in range(N):
        for x in range(N):
            v = enc(m[y][x])
            im.putpixel((x, y), (v, v, v, int(round(255 * a[y][x]))))
    buf = io.BytesIO()
    im.save(buf, "PNG", optimize=True)
    path = os.path.join(OUT, name)
    for _ in range(20):
        try:
            with open(path, "wb") as f:
                f.write(buf.getvalue())
            return im
        except OSError:
            time.sleep(1)
    raise SystemExit("could not write " + path)


def vnoise(rng, cells, n):
    """Smooth value noise 0..1 that tiles over n (cells must divide n)."""
    g = [[rng.random() for _ in range(cells)] for _ in range(cells)]
    o = [[0.0] * n for _ in range(n)]
    for y in range(n):
        for x in range(n):
            fx, fy = x * cells / n, y * cells / n
            x0, y0 = int(fx) % cells, int(fy) % cells
            tx, ty = fx - int(fx), fy - int(fy)
            tx, ty = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
            a = g[y0][x0] * (1 - tx) + g[y0][(x0 + 1) % cells] * tx
            b = g[(y0 + 1) % cells][x0] * (1 - tx) + g[(y0 + 1) % cells][(x0 + 1) % cells] * tx
            o[y][x] = a * (1 - ty) + b * ty
    return o


def depths(x, y):
    """How far a texel is from each edge, and from the nearest one."""
    t, l, b, r = y, x, N - 1 - y, N - 1 - x
    return t, l, b, r, min(t, l, b, r)


def lit_side(x, y):
    """True where the nearest edge is the top or the left.

    Light falls from the upper left in this interface -- the bevel on
    every slab and well already says so -- and a picture that lit the
    other two sides would read as a hole where a slab is meant to be.
    """
    t, l, b, r, d = depths(x, y)
    return d == t or d == l


def field(rng, seed_cells, amount, period):
    """A tileable grain, 1.0 in the mean.

    `period` is the run the picture has to repeat over: the middle of a
    nine-slice is tiled across a panel, so a grain that does not close
    on itself draws a seam every tile. See widgets::Skin::nine.
    """
    v = vnoise(rng, seed_cells, period)
    return [[1.0 + (v[y % period][x % period] - 0.5) * 2.0 * amount for x in range(N)] for y in range(N)]


# ---------------------------------------------------------------- panel

def panel():
    """Tanned hide over a frame: a lip, a stitched channel, four rivets.

    Eight texels of border, which is what the stitching needs to be a
    row of stitches rather than a dotted line, and a middle of sixteen
    that tiles.
    """
    rng = random.Random(7401)
    grain = field(rng, 4, 0.085, 16)
    pores = vnoise(random.Random(7402), 8, 16)
    m = blank()
    a = blank(1.0)
    for y in range(N):
        for x in range(N):
            t, l, b, r, d = depths(x, y)
            lit = lit_side(x, y)
            v = grain[y][x]
            # The hide's own pores: patches, never single pixels -- a
            # lone dark texel at this size reads as dirt on the screen.
            if pores[y % 16][x % 16] < 0.30:
                v *= 0.90
            if d == 0:
                v = 0.22            # the panel's own edge, against the world
            elif d == 1:
                v = 2.05 if lit else 0.40
            elif d == 2:
                v = 1.35 if lit else 0.72
            elif d in (4, 5):
                # The stitch channel. Two texels on, two off: a period
                # of four divides the sixteen-texel edge strip, so the
                # thread carries on across a tile boundary instead of
                # stuttering. The thread sits proud on its upper row and
                # shaded on its lower one, which is what makes it a
                # thread rather than a dash.
                along = x if (d == t or d == b) else y
                v = (1.80 if d == 4 else 1.20) if along % 4 < 2 else 0.52
            elif d == 6:
                v = 1.18 if lit else 0.84
            m[y][x] = v
    # Rivets in the corners, on the inner side of the stitching: a bright
    # head, a shadow under it, on a pad that lifts it off the hide.
    for cy in (4, N - 5):
        for cx in (4, N - 5):
            for y in range(cy - 2, cy + 3):
                for x in range(cx - 2, cx + 3):
                    dd = math.hypot(x - cx, y - cy)
                    if dd <= 1.2:
                        m[y][x] = 2.35
                    elif dd <= 2.1:
                        m[y][x] = 0.45 if (x >= cx and y >= cy) else 1.30
    return m, a


# ------------------------------------------------------------ hollows

def hollow(floor, lip_dark, lip_light, border, grain_amount, seed):
    """A well cut into the surface: dark where the lip shades it.

    The mean stays at 1.0 on purpose. Every contrast in this interface
    is measured against the theme's own numbers (`Theme::STONE`), and a
    skin whose mean is not neutral would make every one of those
    measurements a lie.
    """
    rng = random.Random(seed)
    grain = field(rng, 4, grain_amount, N - 2 * border)
    m = blank()
    a = blank(1.0)
    for y in range(N):
        for x in range(N):
            t, l, b, r, d = depths(x, y)
            lit = lit_side(x, y)
            if d == 0:
                v = 0.92
            elif d == 1:
                v = lip_dark if lit else lip_light
            elif d == 2:
                v = (lip_dark + 1.0) / 2.0 if lit else (lip_light + 1.0) / 2.0
            else:
                v = floor * grain[y][x]
            m[y][x] = v
    return m, a


# --------------------------------------------------------------- slots

def slot(floor, lip_dark, lip_light, seed, scuff):
    """One cell, drawn whole rather than sliced.

    A slot is always about square and always about the same size, so it
    is one quad with the whole picture on it -- five times cheaper than
    a nine-slice and, at this size, indistinguishable. `Painter::well`
    is what a *stretched* hollow goes through.
    """
    rng = random.Random(seed)
    grain = vnoise(rng, 8, N)
    m = blank()
    a = blank(1.0)
    for y in range(N):
        for x in range(N):
            t, l, b, r, d = depths(x, y)
            lit = lit_side(x, y)
            if d == 0:
                v = 0.50            # the seam between one cell and the next
            elif d in (1, 2):
                v = lip_dark if lit else lip_light
            elif d == 3:
                v = 0.86
            else:
                # The floor, with a vignette: darker into the corners,
                # so a grid of these reads as forty holes rather than
                # forty tiles.
                dx = (x - 15.5) / 12.0
                dy = (y - 15.5) / 12.0
                v = floor * (1.0 - 0.13 * min(1.0, dx * dx + dy * dy))
                v *= 1.0 + (grain[y][x] - 0.5) * 2.0 * 0.05
                # A few scuffs where things have been dropped in, in
                # patches rather than as single texels: a lone dark
                # pixel at this size is a dead sub-pixel, not a scuff.
                if scuff and grain[y][x] < 0.24:
                    v *= 0.90
            m[y][x] = v
    return m, a


def slot_hover():
    """The glow under the pointer: an overlay, not a second cell.

    Alpha only where it adds something, so it can be laid over the plain
    cell without redrawing it. A second opaque picture would have to
    agree with the first one about where the lip is, and two pictures
    that have to agree are two pictures that stop agreeing.
    """
    m = blank()
    a = blank(0.0)
    for y in range(N):
        for x in range(N):
            _, _, _, _, d = depths(x, y)
            if d in (1, 2, 3):
                m[y][x] = 1.15 if d == 3 else 1.0
                a[y][x] = 0.50 if d == 3 else 0.30
            elif d > 3:
                m[y][x] = 1.0
                a[y][x] = 0.15
    return m, a


def slot_selected():
    """The chosen cell: a ring of thread round the inside of the lip.

    Drawn in the accent, which is the one colour in this game that is
    not grey, and only on the ring -- an amber wash over the whole cell
    would hide whatever is lying in it, which is the one thing the cell
    is for.
    """
    m = blank()
    a = blank(0.0)
    for y in range(N):
        for x in range(N):
            t, l, b, r, d = depths(x, y)
            if d == 1:
                m[y][x] = 1.05
                a[y][x] = 0.95
            elif d == 2:
                m[y][x] = 0.80
                a[y][x] = 0.85
            elif d == 3:
                along = x if (d == t or d == b) else y
                m[y][x] = 1.00
                a[y][x] = 0.70 if along % 4 < 2 else 0.28
            elif d in (4, 5):
                m[y][x] = 0.85
                a[y][x] = 0.10
    return m, a


def slot_blocked():
    """A cell nothing may go in: hatched, not merely dimmed.

    Dimming is what a disabled button does, and a dimmed cell in a grid
    of cells reads as an empty one. Hatching reads as "not here" at a
    glance and survives whatever is drawn underneath.
    """
    m = blank()
    a = blank(0.0)
    for y in range(N):
        for x in range(N):
            _, _, _, _, d = depths(x, y)
            if d < 2:
                continue
            if (x + y) % 7 < 2:
                m[y][x] = 0.30
                a[y][x] = 0.55
            elif d >= 3:
                m[y][x] = 0.55
                a[y][x] = 0.22
    return m, a


# ------------------------------------------------------------- raised

def board(face, lip_light, lip_dark, seed, pressed=False, bottom_line=False, open_bottom=False):
    """A board standing proud of the panel: what a button is.

    `pressed` turns the bevel over. That is the whole of what a pressed
    button is -- the light moves to the other two sides -- and it is
    worth more than darkening the face, because a player can see it out
    of the corner of an eye.
    """
    rng = random.Random(seed)
    grain = field(rng, 4, 0.06, 20)
    # Straight streaks a row at a time, and most rows get none: a board
    # with a stripe on every row is a barcode, which is what the first
    # cut of this looked like.
    streak = [1.0 + (rng.uniform(-0.05, 0.05) if rng.random() < 0.45 else 0.0) for _ in range(20)]
    m = blank()
    a = blank(1.0)
    for y in range(N):
        for x in range(N):
            t, l, b, r, d = depths(x, y)
            lit = lit_side(x, y) != pressed
            v = face * grain[y][x]
            # The grain runs across the board, the way a plank is sawn:
            # straight streaks a row at a time, never a wave. A sine in a
            # texture is the one thing the player has named twice ("what
            # is this wavy texture?"), and a board does not ripple.
            v *= streak[y % 20]
            if d == 0:
                v = 0.26 if not open_bottom or d != b else face
            elif d == 1:
                v = lip_light if lit else lip_dark
            elif d == 2:
                v = (lip_light + face) / 2.0 if lit else (lip_dark + face) / 2.0
            if bottom_line and b <= 1:
                v = 0.30
            m[y][x] = v
    return m, a


def grip():
    """The scrollbar's thumb: a raised strap with three notches.

    Notches rather than a plain block, because a thumb with a grip on it
    reads as something to drag. The notches sit in the middle sixteen
    texels so that they tile with the strap when it is long.
    """
    m, a = board(1.05, 1.85, 0.48, 5511)
    for y in range(N):
        for x in range(N):
            if 6 <= x <= N - 7 and (y - 16) % 5 in (0, 1) and 12 <= y <= 21:
                m[y][x] *= 0.55 if (y - 16) % 5 == 0 else 1.45
    return m, a


def rule():
    """A hairline divider, drawn at a fixed height and stretched sideways.

    Every row is uniform, so stretching it along a panel cannot smear
    anything: the picture has no detail in the direction it is stretched.
    That is why this is one quad rather than a nine-slice.
    """
    m = blank()
    a = blank(0.0)
    for y in range(N):
        v, alpha = 1.0, 0.0
        if y in (14, 15):
            v, alpha = 0.34, 0.85
        elif y in (16, 17):
            v, alpha = 1.90, 0.55
        m[y] = [v] * N
        a[y] = [alpha] * N
    return m, a


def main():
    os.makedirs(OUT, exist_ok=True)
    pieces = {
        "panel.png": panel(),
        # A shallow tray: a group of slots stands in one, and it has to
        # read as a different *place* from the panel without reading as
        # a hole. See `Theme::tray`.
        "tray.png": hollow(1.0, 0.62, 1.45, 6, 0.05, 3101),
        "well.png": hollow(1.0, 0.36, 1.80, 6, 0.06, 3102),
        "slot.png": slot(1.0, 0.36, 1.78, 3201, True),
        "slot_hover.png": slot_hover(),
        "slot_selected.png": slot_selected(),
        "slot_blocked.png": slot_blocked(),
        "button.png": board(1.0, 1.95, 0.44, 4101),
        "button_hover.png": board(1.14, 2.20, 0.52, 4102),
        "button_down.png": board(0.82, 1.55, 0.38, 4103, pressed=True),
        "tab_on.png": board(1.10, 2.05, 0.46, 4201, open_bottom=True),
        "tab_off.png": board(0.86, 1.45, 0.40, 4202, bottom_line=True),
        "track.png": hollow(1.0, 0.44, 1.55, 6, 0.04, 3103),
        "grip.png": grip(),
        "rule.png": rule(),
    }
    images = []
    for name, (m, a) in pieces.items():
        images.append((name, save(m, a, name)))
        mean = sum(m[y][x] for y in range(N) for x in range(N)) / (N * N)
        print(f"{name:20s} mean multiplier {mean:.3f}")

    # The sheet, at 8x on a grey ground, the way pixel-skill asks for it.
    scale, pad = 8, 10
    cols = 5
    rows = (len(images) + cols - 1) // cols
    size = N * scale
    sheet = Image.new("RGBA", (cols * (size + pad) + pad, rows * (size + pad + 12) + pad), (60, 60, 60, 255))
    for i, (name, im) in enumerate(images):
        cx = (i % cols) * (size + pad) + pad
        cy = (i // cols) * (size + pad + 12) + pad
        sheet.alpha_composite(im.resize((size, size), Image.NEAREST), (cx, cy))
    where = os.environ.get("UI_SKIN_SHEET")
    if where:
        sheet.save(where)
    print("wrote", len(images), "pictures")


if __name__ == "__main__":
    main()
