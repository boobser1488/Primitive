# Pixel-skill: drawing pixel textures in code

This file is a self-contained instruction for an AI agent (Claude, GPT, any
agent that can run Python). Give it to the agent together with a task like
"redraw the bread" or "make a salt marsh soil texture", and the agent can make
16x16 textures in the same style and the same way as the textures in the game
Primitive.

Everything needed is here: the rules, the working order, the helper code and
recipes that were tried out.

---

## 0. What you need to know about the project

- A texture is a **16x16 RGBA PNG**, drawn pixel by pixel.
- Textures live in `assets/textures/<group>/<name>.png`. Groups include
  `terrain`, `rocks`, `soils`, `plants`, `food`, `roof`, `furniture`,
  `metal` and `fire`.
- A new file needs **two lines**:
  - in `assets/textures/blocks.toml`: `name = "group/file.png"` or
    `name = { all = "...", item = "..." }`;
  - in `primitive_client/src/embedded.rs`: an `include_bytes!` entry, modelled
    on its neighbours.
  Redrawing an existing file needs neither.
- The atlas holds 2048 layers. Several `blocks.toml` entries that name the
  same file cost nothing extra.
- Once a file has been added, check it:
  `cargo test -p primitive_client --lib texture:: embedded`.
- **A picture is drawn by a Python + Pillow script**, never by hand and never
  in an editor. That makes the result repeatable: the RNG has a fixed seed, so
  a second run draws the same picture.

---

## 1. The player's rules (every one of these came from real complaints)

1. **Recolour a finished texture first, invent only if that fails.**
   In the player's words: "take the cobblestone, gravel and sand texture and
   recolour it; if it doesn't look realistic, make a new one". The light,
   shade and shape of the stones are taken from the base picture, and only
   the material changes. Bases:
   - rock: `terrain/stone.png`, `terrain/cobblestone.png`,
     `terrain/gravel.png`, `terrain/sand.png`;
   - soil: `terrain/dirt.png`;
   - wood: `terrain/planks.png` and the log sides;
   - items: the copper tools are the style reference.
2. **No tints.** A "real texture" is its own PNG, not a colour multiplier in
   the code.
3. **No lone dots ("peas").** Random single pixels of another colour read as
   peas and look fake. Features go in **patches** (smooth noise with a
   threshold) or in **plates** (Voronoi on a grid). A single pixel is allowed
   only as an intended detail: a crumb of bran, a highlight, a fibre.
4. **No sine waves, arcs or "wavy" bands.** Wavy stripes were rejected ("what
   is this wavy texture?"), and arcs on water looked like "a smiley face".
   Use straight streaks, noise and cracks.
5. **Not too bright.** The player said pale sandstone and limestone cobble
   were "too bright".
   - The middle of stone and cobble is about **140–160 luminance**.
   - For pale materials, cap highlights at `cap = 1.15–1.2` of the mean.
   - Pure white is only for salt, snow, ice and highlights, never a whole
     surface.
6. **Be realistic without leaving the concept.** The pixel style stays, and
   the material has to be recognisable: permafrost has flat ice lenses, a salt
   marsh has a salt crust with cracks, dung has undigested grass.
7. **When the player gives a reference photo, follow it without copying it
   exactly.** Pine bark, for example, became dark vertical one-pixel streaks.
8. **Show before and after.** Always put both on one sheet at 8x and give
   them to the player.

---

## 2. Working order

1. **Look at the old picture.** Make `x_before.png` at 8x on a grey ground
   and look at it. If you can see images, look.
2. **Write the script to a file.** Don't paste it into a shell heredoc:
   apostrophes in comments break bash.
3. **Fix the seeds** with `random.Random(<number>)`.
4. **Draw** following the rules and recipes below.
5. **Save with retries.** A running build can hold the file open, so
   `save()` tries 20 times at 3 s intervals.
6. **Make `x_after.png`, look at it at 8x and judge it honestly.**
   - Can you tell what material it is?
   - Are there peas, stripes or waves?
   - Is it too bright?
   - Does the edge tile with the neighbouring block?
7. **Iterate one or two times.** In a report, write plainly what is still
   not good.
8. **Hand over before and after.** If you added or renamed a file, run the
   test from section 0.

---

## 3. Helper library (texlib.py)

Put it next to the script, or in `.claude/skills/pixel-textures/texlib.py`,
where it already exists in this repo.

```python
from PIL import Image
import random, math, time, io

T = 'assets/textures/'
N = 16

def lum(p):
    return 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]

def clamp(v):
    return int(max(0, min(255, round(v))))

def load(path):
    return Image.open(T + path).convert('RGBA')

def recolour(base, mid, cap=1.2, floor=0.0):
    """Keep the base picture's light and shade, swap the material.
    pixels[y][x] = [r,g,b]; rel[y][x] = brightness relative to the mean."""
    px = base.load(); w, h = base.size
    mean = sum(lum(px[x, y]) for y in range(h) for x in range(w)) / (w * h)
    out = [[None] * w for _ in range(h)]; rel = [[0.0] * w for _ in range(h)]
    for y in range(h):
        for x in range(w):
            r = max(floor, min(cap, lum(px[x, y]) / mean))
            rel[y][x] = r
            out[y][x] = [mid[i] * r for i in range(3)]
    return out, rel

def vnoise(rng, cells, n=N):
    """Smooth value noise 0..1 that tiles (cells must divide n: 2, 4, 8)."""
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

def patches(rng, cells, lo, hi, n=N):
    """Mask of soft patches. cells=4: big patches, cells=8: small ones."""
    v = vnoise(rng, cells, n)
    return [[lo <= v[y][x] < hi for x in range(n)] for y in range(n)]

def plates(rng, per_row=3, n=N):
    """Tiling Voronoi plates on a jittered grid. Plain random points give
    long diagonal slivers at 16 px, so don't use them.
    owner[y][x] = plate number, edge[y][x] = distance to the border (small = crack)."""
    step = n / per_row
    pts = [((i % per_row) * step + rng.uniform(0.1, 0.9) * step,
            (i // per_row) * step + rng.uniform(0.1, 0.9) * step) for i in range(per_row * per_row)]
    owner = [[0] * n for _ in range(n)]; edge = [[0.0] * n for _ in range(n)]
    for y in range(n):
        for x in range(n):
            d = sorted((math.hypot(x + 0.5 - px - ox, y + 0.5 - py - oy), i)
                       for i, (px, py) in enumerate(pts) for ox in (-n, 0, n) for oy in (-n, 0, n))
            owner[y][x] = d[0][1]; edge[y][x] = d[1][0] - d[0][0]
    return owner, edge

def blend(c, col, a, shade=1.0):
    """Mix colour col over c with strength a; shade carries the base's light."""
    return [c[i] * (1 - a) + col[i] * shade * a for i in range(3)]

def to_image(pix):
    h, w = len(pix), len(pix[0]); im = Image.new('RGBA', (w, h))
    for y in range(h):
        for x in range(w):
            im.putpixel((x, y), (0, 0, 0, 0) if pix[y][x] is None
                        else tuple(clamp(v) for v in pix[y][x][:3]) + (255,))
    return im

def save(im, path):
    buf = io.BytesIO(); im.save(buf, 'PNG')
    for _ in range(20):
        try:
            open(T + path, 'wb').write(buf.getvalue()); return
        except OSError:
            time.sleep(3)
    raise SystemExit('could not write ' + path)

def sheet(paths, out, scale=8):
    """Preview at 8x on a grey ground, several pictures side by side."""
    size = N * scale
    sh = Image.new('RGBA', (len(paths) * (size + 12), size + 12), (60, 60, 60, 255))
    for i, p in enumerate(paths):
        im = load(p) if isinstance(p, str) else p
        sh.alpha_composite(im.resize((size, size), Image.NEAREST), (i * (size + 12) + 6, 6))
    sh.save(out)
```

---

## 4. Recipes that were accepted

### 4.1 Rock (andesite, granite, marble and others)

```python
rng = random.Random(sum(map(ord, 'granite')))
pix, rel = recolour(load('terrain/cobblestone.png'), (176, 146, 134))
for mask, col, a in [(patches(rng, 8, 0.0, 0.22), (214, 150, 132), 0.6),   # pink feldspar
                     (patches(rng, 8, 0.0, 0.18), (40, 36, 36), 0.65),     # dark mica
                     (patches(rng, 8, 0.8, 1.0), (210, 208, 204), 0.5)]:   # quartz
    for y in range(N):
        for x in range(N):
            if mask[y][x] and rel[y][x] >= 0.72:      # the gaps between stones stay gaps
                pix[y][x] = blend(pix[y][x], col, a, min(1.25, max(0.75, rel[y][x])))
save(to_image(pix), 'rocks/granite_cobble.png')
```

- Each rock comes in four forms: stone from `stone`, cobble from
  `cobblestone`, gravel from `gravel`, and sand from `sand`.
- The features are the same in all four. Their strength is 1.0 for stone and
  cobble, 0.7 for gravel and 0.35 for sand.
- For sand, blend the colour 30% toward (214, 196, 158).
- Layered rocks (shale, sandstone) get straight broken layers, for example
  `y % 5 == 2 and (x + y) % 6 < 4`. Never a sine.

### 4.2 Soil

Recolour `terrain/dirt.png` with `cap = 1.15–1.25`, then add one or two
layers of patches:

- **Chernozem:** middle (52, 40, 32), with dark and slightly lighter patches.
- **Podzol:** a grey-white bleached layer across the top rows, a rust-coloured
  band under it, brown below.
- **Solonchak (salt marsh):** `plates(rng, 3)`.
  - Most plates get a salt crust, blended toward (226, 224, 214) with strength
    0.5–0.72.
  - Cracks where `edge <= 0.8` are darkened ×0.72.
  - Two plates are left as bare soil (104, 92, 78).
- **Permafrost:** cold grey-brown soil (86, 80, 74) with patches of hoar frost
  (170, 182, 190) at a=0.35.
  - Ice lenses are short straight horizontal streaks, 3–7 px long.
  - Core (196, 214, 224) with bright pixels (226, 238, 244); ends (160, 176, 186).
  - The row under a lens is darkened ×0.8.
- **Rendzina:** dark soil with big chalk patches (`cells = 4`), not dots.

### 4.3 An item on a transparent background (food, clumps)

A shape from a mask, light from the upper left, a palette of 5 shades from
dark to light:

```python
def lump(img, rng, cx, cy, rx, ry, palette, top_flat=0.0, jitter=0.3, rim=2):
    for y in range(N):
        for x in range(N):
            dx = (x + 0.5 - cx) / rx; dy = (y + 0.5 - cy) / ry
            d = dx * dx + dy * dy + 0.06 * math.sin(x * 1.7 + y * 0.9)   # a slightly uneven edge
            if d > 1.0:
                continue
            nz = math.sqrt(max(0.0, 1 - d)) * (1 - top_flat)
            light = 0.5 * nz - 0.35 * dx - 0.45 * dy + 0.2
            k = int(max(0, min(len(palette) - 1, 1.3 + light * 3.0 + rng.uniform(-jitter, jitter))))
            if d > 0.8 and dy > 0.1:
                k = max(0, k - rim)              # darker lower rim: the thing sits on something
            img.putpixel((x, y), palette[k] + (255,))
```

Then add 3–6 **meaningful** details. These made the result:

- **Bread:** a round loaf. The crust palette runs (92,52,22) → (200,150,84).
  Three slanted score cuts of light crumb (232,200,140) with a dark edge
  beside them, and a few specks of flour.
- **Dough:** a cream ball squashed a little (`top_flat=0.2`), a fold of 4
  darker pixels, and flour dust on top.
- **Flour:** a bell-shaped heap, `h = 13.5 - 8*exp(-((x-8)/4.2)^2)`. The
  left slope is lit and the right is in shade, the grain is fine noise, flour
  spills at the foot, and 4 specks of bran (150,124,90).
- **Fat:** a waxy off-white lump (`top_flat=0.35`), a pink thread of meat
  (190,116,108), a highlight of 3 px at the upper left, and lumps inside.
- **Moss:** a cushion with a wavy upper edge. The palette goes from dark
  green at the bottom to light green at the top, single-pixel fronds stick up
  with light tips, and soil and rootlets hang underneath.

### 4.4 A block icon (steps, slabs)

Take an existing icon of the right shape. Keep its alpha and its brightness
relative to its mean, and fill it from the material's picture:

```python
l = lum(shape_px) / mean_of_shape
l = 1.0 + (l - 1.0) * 0.6            # soften: the source icon's pattern is not the new material's
colour = material_px[x % 16, y % 16] * clamp(l, 0.45, 1.2)
```

### 4.5 Bark and wood

- Recolour the log side. Don't draw a wavy pattern.
- Pine: dark vertical one-pixel streaks of uneven length on a red-brown
  ground. Scales are plates, not dots.
- Birch: a light ground with short horizontal dark dashes (lenticels).

---

## 5. Common mistakes and how to avoid them

| Symptom | Cause | Fix |
|---|---|---|
| "Peas" | `rng.random() < p` per pixel | `patches()` with `cells = 4/8` |
| Diagonal slivers | Voronoi on random points | `plates()` on a grid |
| Wavy bands | a `sin` in the pattern | straight streaks, noise |
| "Too bright" | pale middle and no cap | middle 140–160, `cap = 1.15` |
| Looks like a tint | only the colour changed | add the material's features: cracks, lenses, fibres |
| A seam between blocks | noise does not tile | `cells` divides 16; wrap Voronoi by ±16 |
| Picture "floats" | no rim shadow | darken the lower edge (`rim = 2`) |
| File not written | a build holds it | `save()` with retries |

---

## 6. Report to the player

Keep it short, in the player's language:

1. what you redrew and what the picture now shows, in one line per texture;
2. the before and after sheet;
3. what is still not good, if anything;
4. whether you added or renamed files, and whether the texture tests passed.
