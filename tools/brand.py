#!/usr/bin/env python3
"""The brand sheet: three marks, one geometry, and a renderer for all."""

import math
import struct
import zlib
from array import array


STROKE = 2.0

MARKS = {
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
        {
            "d": (
                "M 12 13.1 A 1.7 1.7 0 0 0 11.1 16.25 L 10.7 18.4 "
                "H 13.3 L 12.9 16.25 A 1.7 1.7 0 0 0 12 13.1 Z"
            ),
            "fill": True,
        },
    ],
    "hearth": [
        {"d": "M 4 20.5 V 12 A 8 8 0 0 1 20 12 V 20.5", "stroke": STROKE},
        {"d": "M 2.5 20.5 H 21.5", "stroke": STROKE},
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
        {
            "d": "M 10.4 10.4 L 14.6 12.5 L 10.4 14.6 Z",
            "fill": True,
        },
    ],
}


INK = (0x1B, 0x1D, 0x20)
PAPER = (0xF2, 0xF4, 0xF6)

TILE_HALF = 0.461
TILE_RADIUS = 0.198
MARK_SPAN = 0.64


def _numbers(text):
    """Every number in a path data string, in order."""
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
    """Path data to a list of subpaths."""
    subpaths = []
    points = []
    closed = False
    start = (0.0, 0.0)
    cursor = (0.0, 0.0)
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


def _split_long(points, limit):
    """Break long straight runs up so each segment has a tight bounding box."""
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
    """Set every sample within `half` of the segment p0-p1."""
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
    """Render `shapes` into an n-by-n coverage mask of 0/1 bytes."""
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
    """Render one named mark on its tile at each requested size."""
    shapes = MARKS[mark]
    span = MARK_SPAN / 24.0
    origin = (1.0 - MARK_SPAN) / 2.0

    def place(x, y):
        return (origin + x * span, origin + y * span)

    tile = rasterize([{"d": _rounded_rect_path(TILE_HALF, TILE_RADIUS), "fill": True}], master)
    glyph = rasterize(shapes, master, place)
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
                r = (PAPER[0] * m + INK[0] * (a - m)) / a
                g = (PAPER[1] * m + INK[1] * (a - m)) / a
                b = (PAPER[2] * m + INK[2] * (a - m)) / a
                row += bytes((round(r), round(g), round(b), round(a * 255)))
            rows.append(bytes(row))
        out[size] = rows
    return out


def render_glyph(mark, sizes, colour=PAPER, span=0.46, master=2048):
    """Render the mark alone, on transparency."""
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
    """The mark on its own, as an SVG document."""
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
