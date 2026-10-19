import math, zlib, struct

W = H = 512
SS = 2  # supersample factor for extra smoothing

def sd_round_rect(px, py, cx, cy, hw, hh, r):
    qx = abs(px - cx) - (hw - r)
    qy = abs(py - cy) - (hh - r)
    return math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - r

def cov(d):
    # d in pixels, negative inside. 1px antialias band
    return min(1.0, max(0.0, 0.5 - d))

def lerp(a, b, t):
    return a + (b - a) * t

def shade(x, y):
    # background gradient (top-left indigo -> bottom-right violet)
    t = (x / W * 0.45 + y / H * 0.55)
    r = lerp(74, 126, t)
    g = lerp(105, 58, t)
    b = lerp(250, 232, t)

    # rounded square background
    d_bg = sd_round_rect(x, y, W / 2, H / 2, W / 2 - 6, H / 2 - 6, 116)
    a_bg = cov(d_bg)
    if a_bg <= 0:
        return (0, 0, 0, 0)

    # ---- glyph: RJ45 style port ----
    # outer frame ring
    outer = sd_round_rect(x, y, 256, 268, 108, 78, 26)
    inner = sd_round_rect(x, y, 256, 268, 108 - 27, 78 - 27, 12)
    ring = max(outer, -inner)

    # teeth (contact pins) hanging from the top edge inside the port
    teeth = 1e9
    for cx in (196, 240, 284, 328):
        d = sd_round_rect(x, y, cx, 218, 9, 20, 7)
        teeth = min(teeth, d)

    glyph = min(ring, teeth)
    a_g = cov(glyph)

    # small node dot bottom center for a hint of "network socket"
    ic = math.hypot(x - 256, y - 306)
    a_g = max(a_g, cov(ic - 17))

    # composite white over gradient
    r = lerp(r, 255, a_g)
    g = lerp(g, 255, a_g)
    b = lerp(b, 255, a_g)
    return (int(r), int(g), int(b), int(a_bg * 255))

def render():
    # render at SS scale then box filter
    big = []
    for y in range(H * SS):
        row = []
        for x in range(W * SS):
            row.append(shade((x + 0.5) / SS, (y + 0.5) / SS))
        big.append(row)
    out = bytearray()
    for y in range(H):
        out.append(0)
        for x in range(W):
            acc = [0, 0, 0, 0]
            for dy in range(SS):
                for dx in range(SS):
                    p = big[y * SS + dy][x * SS + dx]
                    for i in range(4):
                        acc[i] += p[i]
            n = SS * SS
            out.extend(acc[i] // n for i in range(4))
    return bytes(out)

raw = render()
def chunk(tag, data):
    c = struct.pack(">I", len(data)) + tag + data
    c += struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    return c

png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(raw, 9))
png += chunk(b"IEND", b"")
open("app-icon.png", "wb").write(png)
print("written", len(png))
