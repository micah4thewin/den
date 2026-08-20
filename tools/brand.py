#!/usr/bin/env python3
"""The brand sheet: three marks, one geometry, and a renderer for all.

This file is byte-identical in the lockbox, hearth, and den repositories. That is
the point of it. The two applications are made by the same hand, and the
cheapest way to say so and keep saying so is for the marks to come off one
sheet that either repository can regenerate and anybody can diff.

    python3 tools/generate_icons.py        # in either repository

# What is here

`MARKS` holds both marks as SVG path data on a 24x24 grid -- the same grid,
the same 2.0 stroke, the same round caps and joins as every glyph in the
interface icon set (lockbox: apps/desktop/src/ui/icons.ts, hearth:
src/js/icons.js). The application icon is therefore not a picture that
resembles the interface; it is the interface's own glyph, rasterized larger.
`tools/check_brand.py` fails the build if the two ever drift apart.

Each mark is a container holding one emblem, because that is what both
programs are:

    lockbox   a box, closed, with a keyhole      a place things are kept
    hearth    an arch, open, with a flame        a place things are done
    den       a CRT, with a play triangle       a place games are played

# Why a rasterizer lives in a repository

An icon is the one asset a reader cannot diff. A repository that ships
binaries nobody can regenerate accumulates files whose provenance is a
shrug. Pure standard library, so regenerating needs no wheel, no lockfile,
and no network: PNG is a signature, three chunks and a CRC; ICO and ICNS
are containers around those PNGs; and the path renderer below is a few
hundred lines because the alternative is depending on Cairo to draw eight
shapes.

The renderer is deliberately small rather than general. It reads the subset
of SVG path syntax the marks use, flattens curves to polylines, and fills
by scanline (non-zero winding) or strokes by capsule coverage. It has no
opinion about colour, gradients, dash patterns, or miter joins, because the
brand has none either.
"""

import math
import struct
import zlib
from array import array

# ---------------------------------------------------------------------------
# The marks
# ---------------------------------------------------------------------------

# Both marks are drawn in a 24x24 box, stroked at 2.0 with round caps and
# joins, and are symmetric about x = 12. Where a shape is filled instead of
# stroked it is because the shape is small enough that an outline would
# close up into a blob at 32px -- the keyhole and the flame are emblems, not
# outlines, and they carry the mark at icon sizes.
#
# Keep every coordinate on a half-unit where the drawing allows it. Marks
# that land on the grid stay crisp when a platform rasterizes them at a size
# nobody here anticipated.

STROKE = 2.0

MARKS = {
    # A padlock: a rounded body, a shackle over it, a keyhole in it.
    #
    # The body is drawn as an explicit rounded rectangle rather than a
    # <rect rx>, so the same path data serves the interface glyph and the
    # renderer below without either needing to know what a rect is.
    "lockbox": [
        {
            "d": (
                "M 6.5 10 H 17.5 A 2.5 2.5 0 0 1 20 12.5 V 18.5 "
                "A 2.5 2.5 0 0 1 17.5 21 H 6.5 A 2.5 2.5 0 0 1 4 18.5 "
                "V 12.5 A 2.5 2.5 0 0 1 6.5 10 Z"
            ),
            "stroke": STROKE,
        },
        {"d": "M 8 10 V 7 A 4 4 0 0 1 16 7 V 10", "stroke": STROKE},
        # The keyhole, filled: a bore with a tapering ward below it. One
        # closed path so it reads as a single cut rather than two marks.
        {
            "d": (
                "M 12 13.1 A 1.7 1.7 0 0 0 11.1 16.25 L 10.7 18.4 "
                "H 13.3 L 12.9 16.25 A 1.7 1.7 0 0 0 12 13.1 Z"
            ),
            "fill": True,
        },
    ],
    # A hearth: the arch of the opening, the floor it stands on, a fire in it.
    #
    # The arch is a true semicircle (chord 16, radius 8) on straight legs,
    # so it is the same family of shape as the padlock body above -- a
    # container, closed at the top, open where you reach into it.
    "hearth": [
        {"d": "M 4 20.5 V 12 A 8 8 0 0 1 20 12 V 20.5", "stroke": STROKE},
        {"d": "M 2.5 20.5 H 21.5", "stroke": STROKE},
        # The flame, filled and symmetric. A leaning flame is warmer and
        # every bit as legible, but it would be the only asymmetric thing
        # either mark contains, and the pair is quieter without it.
        #
        # The tip matters more than the body. Drawn with the control points
        # close to the axis it comes to a real point, roughly 28 degrees,
        # and the shape reads as fire; drawn with them further out the
        # curve closes over into a dome and the same shape reads as a
        # water drop, which is a hard thing to unsee once seen.
        {
            "d": (
                "M 12 7.8 C 12.9 10.6 15.5 12.9 15.5 16.2 "
                "C 15.5 18.1 14.1 19.6 12 19.6 "
                "C 9.9 19.6 8.5 18.1 8.5 16.2 "
                "C 8.5 12.9 11.1 10.6 12 7.8 Z"
            ),
            "fill": True,
        },
    ],
    "den": [
        # A CRT television: a rounded body, two antenna stubs, and a play
        # triangle on the glass. The body is the container, the triangle is
        # the emblem, exactly like the keyhole in the lockbox body and the
        # flame under the hearth arch.
        {
            "d": (
                "M 5 7 H 19 A 2 2 0 0 1 21 9 V 16 "
                "A 2 2 0 0 1 19 18 H 5 A 2 2 0 0 1 3 16 "
                "V 9 A 2 2 0 0 1 5 7 Z"
            ),
            "stroke": STROKE,
        },
        {"d": "M 9.5 7 V 4", "stroke": STROKE},
        {"d": "M 14.5 7 V 4", "stroke": STROKE},
        # The play triangle, filled and symmetric. It points right, toward
        # the thing that happens when you press it. Like the keyhole and the
        # flame, it is small enough that an outline would close into a blob
        # at icon sizes, so it is filled.
        {
            "d": "M 10.4 10.4 L 14.6 12.5 L 10.4 14.6 Z",
            "fill": True,
        },
    ],
}

# ---------------------------------------------------------------------------
# Tile
# ---------------------------------------------------------------------------

# The two tones. Deliberately not pure black: a #000 icon looks like a hole
# punched in a dark dock. These are the --fg and --page tokens from the
# shared palette, so the icon is made of the interface's own two greys.
INK = (0x1B, 0x1D, 0x20)
PAPER = (0xF2, 0xF4, 0xF6)

# All in canvas units, 0..1 across the icon.
TILE_HALF = 0.461  # half-width of the tile; leaves a hair of padding
TILE_RADIUS = 0.198  # corner radius; ~21% of the tile, the modern squircle
MARK_SPAN = 0.64  # the 24-unit mark box, as a fraction of the canvas


# ---------------------------------------------------------------------------
# Path reading
# ---------------------------------------------------------------------------


def _numbers(text):
    """Every number in a path data string, in order.

    Handles the three things SVG path data does that a naive split does
    not: a minus sign that begins a number rather than separating two, a
    decimal point that begins the *next* number ("1.5.5" is two numbers),
    and exponents.
    """
    out = []
    i, n = 0, len(text)
    while i < n:
        ch = text[i]
        if ch in " ,\t\r\n":
            i += 1
            continue
        start = i
        if ch in "+-":
            i += 1
        seen_dot = False
        while i < n:
            c = text[i]
            if c.isdigit():
                i += 1
            elif c == "." and not seen_dot:
                seen_dot = True
                i += 1
            elif c in "eE" and i > start:
                i += 1
                if i < n and text[i] in "+-":
                    i += 1
            else:
                break
        if i == start:
            raise ValueError(f"not a number at {start}: {text[start:start + 12]!r}")
        out.append(float(text[start:i]))
    return out


def _tokenize(d):
    """Split path data into (command, [numbers]) pairs, expanding repeats."""
    commands = []
    i, n = 0, len(d)
    while i < n:
        if d[i] in " ,\t\r\n":
            i += 1
            continue
        if not d[i].isalpha():
            raise ValueError(f"expected a command at {i}: {d[i:i + 12]!r}")
        letter = d[i]
        i += 1
        start = i
        while i < n and not d[i].isalpha():
            i += 1
        # An arc's flags are single digits and may be written without any
        # separator ("0 0 1 8 0"), which the number reader handles, but a
        # flag glued to the next number ("0011 8") would not round-trip.
        # The marks here always space their arc arguments, and the check
        # below catches it loudly if one ever does not.
        args = _numbers(d[start:i])
        arity = {
            "M": 2, "L": 2, "T": 2, "H": 1, "V": 1,
            "C": 6, "S": 4, "Q": 4, "A": 7, "Z": 0,
        }[letter.upper()]
        if arity == 0:
            commands.append((letter, []))
            continue
        if not args or len(args) % arity:
            raise ValueError(f"{letter} wants a multiple of {arity} arguments, got {len(args)}")
        for k in range(0, len(args), arity):
            # A repeated M continues as L, per the SVG grammar.
            follow = letter
            if k and letter == "M":
                follow = "L"
            elif k and letter == "m":
                follow = "l"
            commands.append((follow, args[k:k + arity]))
    return commands


def _cubic(p0, p1, p2, p3, steps):
    """Flatten one cubic bezier, excluding its first point."""
    out = []
    for s in range(1, steps + 1):
        t = s / steps
        u = 1.0 - t
        a, b, c, e = u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t
        out.append((
            a * p0[0] + b * p1[0] + c * p2[0] + e * p3[0],
            a * p0[1] + b * p1[1] + c * p2[1] + e * p3[1],
        ))
    return out


def _arc(p0, rx, ry, phi_deg, large, sweep, p1, steps):
    """Flatten one elliptical arc, excluding its first point (SVG F.6.5)."""
    x0, y0 = p0
    x1, y1 = p1
    if (x0, y0) == (x1, y1):
        return []
    rx, ry = abs(rx), abs(ry)
    if rx == 0 or ry == 0:
        return [p1]

    phi = math.radians(phi_deg)
    cos_p, sin_p = math.cos(phi), math.sin(phi)
    dx2, dy2 = (x0 - x1) / 2.0, (y0 - y1) / 2.0
    x1p = cos_p * dx2 + sin_p * dy2
    y1p = -sin_p * dx2 + cos_p * dy2

    # Scale the radii up if they are too small to span the endpoints.
    lam = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry)
    if lam > 1:
        s = math.sqrt(lam)
        rx, ry = rx * s, ry * s

    num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p
    den = rx * rx * y1p * y1p + ry * ry * x1p * x1p
    factor = math.sqrt(max(num / den, 0.0))
    if large == sweep:
        factor = -factor
    cxp = factor * rx * y1p / ry
    cyp = -factor * ry * x1p / rx
    cx = cos_p * cxp - sin_p * cyp + (x0 + x1) / 2.0
    cy = sin_p * cxp + cos_p * cyp + (y0 + y1) / 2.0

    def angle(ux, uy, vx, vy):
        dot = ux * vx + uy * vy
        norm = math.hypot(ux, uy) * math.hypot(vx, vy)
        a = math.acos(max(-1.0, min(1.0, dot / norm))) if norm else 0.0
        return -a if ux * vy - uy * vx < 0 else a

    theta = angle(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
    delta = angle(
        (x1p - cxp) / rx, (y1p - cyp) / ry,
        (-x1p - cxp) / rx, (-y1p - cyp) / ry,
    )
    if not sweep and delta > 0:
        delta -= 2 * math.pi
    elif sweep and delta < 0:
        delta += 2 * math.pi

    count = max(steps, int(steps * abs(delta) / math.pi))
    out = []
    for s in range(1, count + 1):
        t = theta + delta * s / count
        ex, ey = rx * math.cos(t), ry * math.sin(t)
        out.append((cos_p * ex - sin_p * ey + cx, sin_p * ex + cos_p * ey + cy))
    return out


def flatten(d, steps=24):
    """Path data to a list of subpaths.

    Each subpath is `(points, closed)`. Points are in the path's own units;
    a closed subpath does not repeat its first point.
    """
    subpaths = []
    points = []
    closed = False
    start = (0.0, 0.0)
    cursor = (0.0, 0.0)
    # The reflected control point for a smooth curve continuation.
    last_control = None
    last_kind = None

    def flush():
        nonlocal points, closed
        if len(points) > 1:
            subpaths.append((points, closed))
        points = []
        closed = False

    for letter, args in _tokenize(d):
        upper = letter.upper()
        relative = letter.islower()
        cx, cy = cursor

        if upper == "M":
            flush()
            x, y = args
            cursor = (cx + x, cy + y) if relative else (x, y)
            start = cursor
            points = [cursor]
            last_control, last_kind = None, "M"
            continue

        if upper == "Z":
            if points:
                closed = True
                cursor = start
                flush()
                points = [start]
            last_control, last_kind = None, "Z"
            continue

        if upper == "L":
            x, y = args
            cursor = (cx + x, cy + y) if relative else (x, y)
            points.append(cursor)
        elif upper == "H":
            x = args[0]
            cursor = (cx + x, cy) if relative else (x, cy)
            points.append(cursor)
        elif upper == "V":
            y = args[0]
            cursor = (cx, cy + y) if relative else (cx, y)
            points.append(cursor)
        elif upper in ("C", "S"):
            if upper == "C":
                x1, y1, x2, y2, x, y = args
                p1 = (cx + x1, cy + y1) if relative else (x1, y1)
                p2 = (cx + x2, cy + y2) if relative else (x2, y2)
            else:
                x2, y2, x, y = args
                p2 = (cx + x2, cy + y2) if relative else (x2, y2)
                if last_kind in ("C", "S") and last_control:
                    p1 = (2 * cx - last_control[0], 2 * cy - last_control[1])
                else:
                    p1 = (cx, cy)
            p3 = (cx + x, cy + y) if relative else (x, y)
            points.extend(_cubic((cx, cy), p1, p2, p3, steps))
            cursor, last_control, last_kind = p3, p2, "C"
            continue
        elif upper in ("Q", "T"):
            if upper == "Q":
                x1, y1, x, y = args
                q = (cx + x1, cy + y1) if relative else (x1, y1)
            else:
                x, y = args
                if last_kind in ("Q", "T") and last_control:
                    q = (2 * cx - last_control[0], 2 * cy - last_control[1])
                else:
                    q = (cx, cy)
            p3 = (cx + x, cy + y) if relative else (x, y)
            # Quadratic to cubic, exactly.
            p1 = (cx + 2.0 / 3.0 * (q[0] - cx), cy + 2.0 / 3.0 * (q[1] - cy))
            p2 = (p3[0] + 2.0 / 3.0 * (q[0] - p3[0]), p3[1] + 2.0 / 3.0 * (q[1] - p3[1]))
            points.extend(_cubic((cx, cy), p1, p2, p3, steps))
            cursor, last_control, last_kind = p3, q, "Q"
            continue
        elif upper == "A":
            rx, ry, rot, large, sweep, x, y = args
            p1 = (cx + x, cy + y) if relative else (x, y)
            points.extend(_arc((cx, cy), rx, ry, rot, int(large), int(sweep), p1, steps))
            cursor, last_control, last_kind = p1, None, "A"
            continue
        else:
            raise ValueError(f"unsupported command {letter}")

        last_control, last_kind = None, upper

    flush()
    return subpaths


# ---------------------------------------------------------------------------
# Rasterizing
# ---------------------------------------------------------------------------


def _split_long(points, limit):
    """Break long straight runs up so each segment has a tight bounding box.

    The stroker walks the bounding box of every segment. For a long
    diagonal that box is mostly empty, and the wasted work grows with the
    square of the length; capping the length keeps it linear.
    """
    if limit <= 0:
        return points
    out = [points[0]]
    for i in range(1, len(points)):
        x0, y0 = out[-1]
        x1, y1 = points[i]
        length = math.hypot(x1 - x0, y1 - y0)
        if length > limit:
            steps = int(length / limit) + 1
            for s in range(1, steps):
                t = s / steps
                out.append((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t))
        out.append((x1, y1))
    return out


def _capsule(mask, n, p0, p1, half):
    """Set every sample within `half` of the segment p0-p1.

    Round caps and round joins both fall out of this for free: a capsule
    is a segment dilated by a disc, and consecutive capsules that share an
    endpoint already overlap on the disc at that point.
    """
    x0, y0 = p0
    x1, y1 = p1
    lo_x = max(0, int(math.floor(min(x0, x1) - half)))
    hi_x = min(n - 1, int(math.ceil(max(x0, x1) + half)))
    lo_y = max(0, int(math.floor(min(y0, y1) - half)))
    hi_y = min(n - 1, int(math.ceil(max(y0, y1) + half)))
    if lo_x > hi_x or lo_y > hi_y:
        return
    dx, dy = x1 - x0, y1 - y0
    length2 = dx * dx + dy * dy
    half2 = half * half
    for py in range(lo_y, hi_y + 1):
        sy = py + 0.5
        row = py * n
        for px in range(lo_x, hi_x + 1):
            sx = px + 0.5
            if length2 == 0.0:
                t = 0.0
            else:
                t = ((sx - x0) * dx + (sy - y0) * dy) / length2
                t = 0.0 if t < 0.0 else (1.0 if t > 1.0 else t)
            ex = sx - (x0 + t * dx)
            ey = sy - (y0 + t * dy)
            if ex * ex + ey * ey <= half2:
                mask[row + px] = 1


def _fill(mask, n, loops):
    """Scanline fill with non-zero winding."""
    edges = []
    for points in loops:
        count = len(points)
        for i in range(count):
            x0, y0 = points[i]
            x1, y1 = points[(i + 1) % count]
            if y0 != y1:
                edges.append((x0, y0, x1, y1))
    if not edges:
        return
    lo_y = max(0, int(math.floor(min(min(e[1], e[3]) for e in edges))))
    hi_y = min(n - 1, int(math.ceil(max(max(e[1], e[3]) for e in edges))))
    for py in range(lo_y, hi_y + 1):
        sy = py + 0.5
        crossings = []
        for x0, y0, x1, y1 in edges:
            if (y0 <= sy < y1) or (y1 <= sy < y0):
                t = (sy - y0) / (y1 - y0)
                crossings.append((x0 + t * (x1 - x0), 1 if y1 > y0 else -1))
        if len(crossings) < 2:
            continue
        crossings.sort()
        row = py * n
        winding = 0
        for i in range(len(crossings) - 1):
            winding += crossings[i][1]
            if winding == 0:
                continue
            lo_x = max(0, int(math.ceil(crossings[i][0] - 0.5)))
            hi_x = min(n - 1, int(math.floor(crossings[i + 1][0] - 0.5)))
            for px in range(lo_x, hi_x + 1):
                mask[row + px] = 1


def _rounded_rect_path(half, radius):
    """The tile, as path data in canvas units."""
    lo, hi = 0.5 - half, 0.5 + half
    r = radius
    return (
        f"M {lo + r} {lo} H {hi - r} A {r} {r} 0 0 1 {hi} {lo + r} "
        f"V {hi - r} A {r} {r} 0 0 1 {hi - r} {hi} "
        f"H {lo + r} A {r} {r} 0 0 1 {lo} {hi - r} "
        f"V {lo + r} A {r} {r} 0 0 1 {lo + r} {lo} Z"
    )


def rasterize(shapes, n, place=None):
    """Render `shapes` into an n-by-n coverage mask of 0/1 bytes.

    `place` maps a point from the shape's own units into canvas units; the
    default treats the shapes as already being in canvas units.
    """
    mask = bytearray(n * n)
    for shape in shapes:
        loops = []
        for points, closed in flatten(shape["d"]):
            if place:
                points = [place(x, y) for x, y in points]
            scaled = [(x * n, y * n) for x, y in points]
            loops.append((scaled, closed))
        if shape.get("fill"):
            _fill(mask, n, [pts for pts, _ in loops])
        else:
            width = shape["stroke"]
            if place:
                # Measure the stroke through the same mapping the points
                # took, so it scales with the mark instead of being a
                # number in the wrong space.
                ax, ay = place(0.0, 0.0)
                bx, by = place(width, 0.0)
                width = math.hypot(bx - ax, by - ay)
            half = width * n / 2.0
            for pts, closed in loops:
                run = list(pts)
                if closed and run[0] != run[-1]:
                    run.append(run[0])
                run = _split_long(run, max(2.0 * half, 1.0))
                for i in range(len(run) - 1):
                    _capsule(mask, n, run[i], run[i + 1], half)
    return mask


def _prefix_rows(mask, n):
    """Row-wise running totals, so a block sum costs one pass per row."""
    sums = array("i", bytes(4 * n * (n + 1)))
    for y in range(n):
        base = y * n
        out = y * (n + 1)
        total = 0
        sums[out] = 0
        for x in range(n):
            total += mask[base + x]
            sums[out + x + 1] = total
    return sums


def _downsample(sums, n, size):
    """Average an n-by-n mask down to size-by-size coverage in 0..1."""
    out = []
    block = n / size
    for py in range(size):
        y0 = int(py * block)
        y1 = int((py + 1) * block)
        row = []
        for px in range(size):
            x0 = int(px * block)
            x1 = int((px + 1) * block)
            total = 0
            for y in range(y0, y1):
                base = y * (n + 1)
                total += sums[base + x1] - sums[base + x0]
            row.append(total / float((y1 - y0) * (x1 - x0)))
        out.append(row)
    return out


def render(mark, sizes, master=2048):
    """Render one named mark on its tile at each requested size.

    Returns `{size: [rows of RGBA bytes]}`. The mask is rasterized once at
    `master` and averaged down, so every size is the same drawing and the
    small ones get a great deal of supersampling for free.
    """
    shapes = MARKS[mark]
    span = MARK_SPAN / 24.0
    origin = (1.0 - MARK_SPAN) / 2.0

    def place(x, y):
        return (origin + x * span, origin + y * span)

    tile = rasterize([{"d": _rounded_rect_path(TILE_HALF, TILE_RADIUS), "fill": True}], master)
    glyph = rasterize(shapes, master, place)
    # The mark never leaves the tile by design; intersecting says so rather
    # than trusting it, and costs one pass.
    for i, value in enumerate(glyph):
        if value and not tile[i]:
            glyph[i] = 0

    tile_sums = _prefix_rows(tile, master)
    glyph_sums = _prefix_rows(glyph, master)

    out = {}
    for size in sizes:
        tile_cov = _downsample(tile_sums, master, size)
        glyph_cov = _downsample(glyph_sums, master, size)
        rows = []
        for y in range(size):
            row = bytearray()
            for x in range(size):
                a = tile_cov[y][x]
                if a <= 0.0:
                    row += bytes(4)
                    continue
                m = min(glyph_cov[y][x], a)
                # Ink and paper tile the same area and never overlap, so the
                # colour is their coverage-weighted mean and the alpha is
                # the tile's own coverage.
                r = (PAPER[0] * m + INK[0] * (a - m)) / a
                g = (PAPER[1] * m + INK[1] * (a - m)) / a
                b = (PAPER[2] * m + INK[2] * (a - m)) / a
                row += bytes((round(r), round(g), round(b), round(a * 255)))
            rows.append(bytes(row))
        out[size] = rows
    return out


# ---------------------------------------------------------------------------
# Containers
# ---------------------------------------------------------------------------


def render_glyph(mark, sizes, colour=PAPER, span=0.46, master=2048):
    """Render the mark alone, on transparency.

    For Android's adaptive icons, whose foreground layer is composited over
    a separately declared background and is then masked to whatever shape
    the launcher prefers -- so the glyph has to sit well inside the frame,
    which `span` (a fraction of the canvas) is for.
    """
    unit = span / 24.0
    origin = (1.0 - span) / 2.0

    def place(x, y):
        return (origin + x * unit, origin + y * unit)

    glyph = rasterize(MARKS[mark], master, place)
    sums = _prefix_rows(glyph, master)

    out = {}
    for size in sizes:
        coverage = _downsample(sums, master, size)
        rows = []
        for y in range(size):
            row = bytearray()
            for x in range(size):
                a = coverage[y][x]
                row += bytes(4) if a <= 0.0 else bytes(colour + (round(a * 255),))
            rows.append(bytes(row))
        out[size] = rows
    return out


def png(rows, size):
    """Encode RGBA rows as a PNG."""

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    raw = b"".join(b"\x00" + row for row in rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico(images):
    """Windows .ico: a directory of embedded PNGs."""
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, payloads = b"", b""
    for size, data in images:
        entries += struct.pack(
            "<BBBBHHII",
            0 if size >= 256 else size,
            0 if size >= 256 else size,
            0, 0, 1, 32,
            len(data),
            offset,
        )
        payloads += data
        offset += len(data)
    return header + entries + payloads


def icns(images):
    """macOS .icns: typed entries, PNG payloads accepted since 10.7."""
    types = {16: b"icp4", 32: b"ic11", 64: b"ic12", 128: b"ic07", 256: b"ic13", 512: b"ic14"}
    body = b""
    for size, data in images:
        tag = types.get(size)
        if tag:
            body += tag + struct.pack(">I", len(data) + 8) + data
    return b"icns" + struct.pack(">I", len(body) + 8) + body


def svg(mark, size=24):
    """The mark on its own, as an SVG document.

    No tile and no colour: `currentColor` means one file serves a light
    interface, a dark one, and a README that does not know which.
    """
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" '
        f'width="{size}" height="{size}" fill="none" stroke="currentColor" '
        f'stroke-width="{STROKE}" stroke-linecap="round" stroke-linejoin="round">'
    ]
    for shape in MARKS[mark]:
        if shape.get("fill"):
            parts.append(f'<path d="{shape["d"]}" fill="currentColor" stroke="none"/>')
        else:
            parts.append(f'<path d="{shape["d"]}"/>')
    parts.append("</svg>")
    return "\n".join(parts) + "\n"
