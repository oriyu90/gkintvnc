#!/usr/bin/env python3
"""Generate GlintVNC app icon (1024x1024 PNG) with no third-party libraries."""
import zlib, struct, math, sys

N = 1024
SS = 2               # 2x2 supersampling for antialiasing
W = N * SS
R = int(0.222 * W)     # corner radius (macOS-ish squircle)
M = 0                  # full-bleed rounded rect

cx, cy = 0.50 * W, 0.52 * W
r_out, r_in = 0.295 * W, 0.056 * W
cx2, cy2 = 0.745 * W, 0.268 * W     # small accent sparkle
r_out2, r_in2 = 0.066 * W, 0.014 * W

def rrect_inside(x, y):
    if x < M or y < M or x >= W - M or y >= W - M:
        return False
    dx = max(M + R - x, 0, x - (W - M - R - 1))
    dy = max(M + R - y, 0, y - (W - M - R - 1))
    # corners: use circle distance
    if x < M + R and y < M + R: return (x - (M + R)) ** 2 + (y - (M + R)) ** 2 <= R * R
    if x > W - M - R - 1 and y < M + R: return (x - (W - M - R - 1)) ** 2 + (y - (M + R)) ** 2 <= R * R
    if x < M + R and y > W - M - R - 1: return (x - (M + R)) ** 2 + (y - (W - M - R - 1)) ** 2 <= R * R
    if x > W - M - R - 1 and y > W - M - R - 1: return (x - (W - M - R - 1)) ** 2 + (y - (W - M - R - 1)) ** 2 <= R * R
    return True

STAR = []
for k in range(8):
    ang = -math.pi / 2 + k * math.pi / 4
    rad = r_out if k % 2 == 0 else r_in
    STAR.append((cx + rad * math.cos(ang), cy + rad * math.sin(ang)))
STAR2 = []
for k in range(8):
    ang = -math.pi / 2 + k * math.pi / 4
    rad = r_out2 if k % 2 == 0 else r_in2
    STAR2.append((cx2 + rad * math.cos(ang), cy2 + rad * math.sin(ang)))

def point_in_poly(px, py, poly):
    inside = False
    j = len(poly) - 1
    for i in range(len(poly)):
        xi, yi = poly[i]; xj, yj = poly[j]
        if (yi > py) != (yj > py):
            xint = (xj - xi) * (py - yi) / (yj - yi) + xi
            if px < xint:
                inside = not inside
        j = i
    return inside

def lerp(a, b, t):
    return tuple(a[i] + (b[i] - a[i]) * t for i in range(3))

BG_TOP, BG_BOT = (37, 22, 60), (18, 10, 32)
GLOW = (88, 40, 140)
STAR_TOP, STAR_BOT = (255, 120, 235), (116, 62, 246)
ACCENT = (140, 235, 255)

rows = []
for y in range(N):
    row = bytearray()
    for x in range(N):
        r_acc = g_acc = b_acc = a_acc = 0.0
        for sy in range(SS):
            for sx in range(SS):
                px = x * SS + (sx + 0.5)
                py = y * SS + (sy + 0.5)
                if not rrect_inside(px, py):
                    continue
                t = (px + py) / (2 * W)
                base = lerp(BG_TOP, BG_BOT, t)
                # purple aurora glow upper-left
                gdx, gdy = px - 0.30 * W, py - 0.28 * W
                gd = math.hypot(gdx, gdy) / (0.95 * W)
                glow = max(0.0, 1.0 - gd) ** 2
                base = tuple(min(255.0, base[i] + GLOW[i] * glow * 0.8) for i in range(3))
                # sparkle halo behind star
                sdx, sdy = px - cx, py - cy
                sd = math.hypot(sdx, sdy) / (r_out * 1.6)
                halo = max(0.0, 1.0 - sd) ** 2
                base = tuple(min(255.0, base[i] + 90 * halo * (0.8 if i == 0 else 0.4)) for i in range(3))
                # main sparkle
                if point_in_poly(px, py, STAR):
                    st = max(0.0, min(1.0, (py - (cy - r_out)) / (2 * r_out)))
                    base = lerp(STAR_TOP, STAR_BOT, st)
                    # white-hot core
                    cs = math.hypot(px - cx, py - cy) / (r_out * 0.52)
                    core = max(0.0, 1.0 - cs) ** 2
                    base = lerp(base, (255, 255, 255), core * 0.85)
                # accent sparkle
                if point_in_poly(px, py, STAR2):
                    st2 = max(0.0, min(1.0, (py - (cy2 - r_out2)) / (2 * r_out2)))
                    base = lerp((225, 255, 255), ACCENT, st2)
                r_acc += base[0]; g_acc += base[1]; b_acc += base[2]; a_acc += 255.0
        cnt = SS * SS
        cov = a_acc / (cnt * 255.0)
        if a_acc == 0:
            row += b'\x00\x00\x00\x00'
        else:
            row += bytes((int(r_acc / cnt), int(g_acc / cnt), int(b_acc / cnt), int(cov * 255)))
    rows.append(bytes(row))

def chunk(typ, data):
    c = struct.pack('>I', len(data)) + typ + data
    return c + struct.pack('>I', zlib.crc32(typ + data) & 0xffffffff)

png = b'\x89PNG\r\n\x1a\n'
png += chunk(b'IHDR', struct.pack('>IIBBBBB', N, N, 8, 6, 0, 0, 0))
raw = b''.join(b'\x00' + r for r in rows)
png += chunk(b'IDAT', zlib.compress(raw, 9))
png += chunk(b'IEND', b'')
out = sys.argv[1]
with open(out, 'wb') as f:
    f.write(png)
print('wrote', out, len(png), 'bytes')
