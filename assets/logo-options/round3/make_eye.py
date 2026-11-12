# BrowseCraft logo, round 3: an eye in front of a monitor; its iris is the globe (real
# coastlines from Natural Earth, public domain), centred on the UK, land in black.
import json, math, sys

LAT0, LON0 = 51.0, -3.0          # globe centred just south-west of the UK
import os
CX, CY, R = (256.0, 256.0, 112.0) if os.environ.get("SMALL") == "1" else (256.0, 222.0, 108.0)  # iris
ZOOM = float(os.environ.get("ZOOM", "1.0"))  # globe radius / iris radius
SMALL = os.environ.get("SMALL") == "1"
data = json.load(open("ne_110m_land.geojson" if SMALL else "ne_50m_land.geojson"))

def proj(lon, lat):
    la, lo = math.radians(lat), math.radians(lon - LON0)
    l0 = math.radians(LAT0)
    x = math.cos(la) * math.sin(lo)
    y = math.cos(l0) * math.sin(la) - math.sin(l0) * math.cos(la) * math.cos(lo)
    c = math.sin(l0) * math.sin(la) + math.cos(l0) * math.cos(la) * math.cos(lo)
    if c < 0:  # behind the globe: onto the edge
        n = math.hypot(x, y) or 1.0
        x, y = x / n, y / n
    return CX + R * ZOOM * x, CY - R * ZOOM * y, c >= 0

def ring_path(ring):
    pts = [proj(lon, lat) for lon, lat in ring]
    if not any(v for _, _, v in pts):
        return None, 0
    xs = [p[0] for p in pts]; ys = [p[1] for p in pts]
    size = max(max(xs) - min(xs), max(ys) - min(ys))
    out, last = [], None
    for x, y, _ in pts:
        s = f"{x:.1f},{y:.1f}"
        if s != last:
            out.append(s); last = s
    return "M" + " L".join(out) + " Z", size

def is_uk(ring):
    lons = [p[0] for p in ring]; lats = [p[1] for p in ring]
    lo = sum(lons) / len(lons); la = sum(lats) / len(lats)
    return -11 < lo < 2.2 and 49.8 < la < 61

land, uk = [], []
for f in data["features"]:
    g = f["geometry"]
    polys = g["coordinates"] if g["type"] == "MultiPolygon" else [g["coordinates"]]
    for poly in polys:
        outer = poly[0]
        d, size = ring_path(outer)
        if d is None or size < 1.2:
            continue
        (uk if is_uk(outer) else land).append(d)

def graticule():
    lines = []
    for lat in range(-60, 90, 30):
        seg = []
        for lon in range(-180, 181, 3):
            x, y, v = proj(lon, lat)
            if v: seg.append(f"{x:.1f},{y:.1f}")
            elif len(seg) > 1: lines.append("M" + " L".join(seg)); seg = []
            else: seg = []
        if len(seg) > 1: lines.append("M" + " L".join(seg))
    for lon in range(-180, 180, 30):
        seg = []
        for lat in range(-90, 91, 3):
            x, y, v = proj(lon, lat)
            if v: seg.append(f"{x:.1f},{y:.1f}")
            elif len(seg) > 1: lines.append("M" + " L".join(seg)); seg = []
            else: seg = []
        if len(seg) > 1: lines.append("M" + " L".join(seg))
    return " ".join(lines)

def sparkle(cx, cy, r, col, op=1.0):
    k = r * 0.22
    return (f'<path d="M{cx},{cy-r} Q{cx+k},{cy-k} {cx+r},{cy} Q{cx+k},{cy+k} {cx},{cy+r} '
            f'Q{cx-k},{cy+k} {cx-r},{cy} Q{cx-k},{cy-k} {cx},{cy-r} Z" fill="{col}" opacity="{op}"/>')

stars = "".join(f'<circle cx="{x}" cy="{y}" r="{r}" fill="#ffffff" opacity="{o}"/>' for x, y, r, o in
    [(58, 46, 2.5, .8), (130, 34, 2, .6), (250, 30, 2, .5), (452, 52, 2.5, .7), (478, 120, 2, .5),
     (38, 150, 2, .5), (36, 420, 2.5, .6), (90, 478, 2, .5), (430, 476, 2, .6), (478, 400, 2, .5)])

EYE = "M24,256 Q256,16 488,256 Q256,496 24,256 Z" if SMALL else "M74,222 Q256,52 438,222 Q256,392 74,222 Z"
ocean, landc, grid = "#2b86b3", "#0b0b0d", "#ffffff"

if SMALL:
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
<!-- BrowseCraft logo, small sizes (16-32 px): the eye and globe alone. Coastlines: Natural Earth (public domain). Original artwork. -->
<defs>
  <clipPath id="eye"><path d="{EYE}"/></clipPath>
  <radialGradient id="sea" cx="0.42" cy="0.38" r="0.75">
    <stop offset="0" stop-color="#4fb2de"/><stop offset="1" stop-color="#1d5f86"/></radialGradient>
</defs>
<rect x="0" y="0" width="512" height="512" rx="104" fill="#10153a"/>
<path d="{EYE}" fill="#f7f4ec"/>
<g clip-path="url(#eye)">
  <circle cx="{CX}" cy="{CY}" r="{R}" fill="url(#sea)"/>
  <path d="{' '.join(land + uk)}" fill="{landc}"/>
</g>
<path d="{EYE}" fill="none" stroke="#0b0f2c" stroke-width="30" stroke-linejoin="round"/>
<circle cx="{CX-50}" cy="{CY-34}" r="22" fill="#ffffff" opacity=".9"/>
</svg>
'''
else:
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
    <!-- BrowseCraft logo option (round 3): an eye in front of a monitor; the iris is the globe,
         centred on the UK. Coastlines: Natural Earth (public domain). Original artwork. -->
    <defs>
      <clipPath id="eye"><path d="{EYE}"/></clipPath>
      <clipPath id="iris"><circle cx="{CX}" cy="{CY}" r="{R}"/></clipPath>
      <linearGradient id="screen" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="#2d4f8c"/><stop offset="1" stop-color="#16274a"/></linearGradient>
      <radialGradient id="sea" cx="0.42" cy="0.38" r="0.75">
        <stop offset="0" stop-color="#4fb2de"/><stop offset="1" stop-color="#1d5f86"/></radialGradient>
    </defs>
    <rect x="16" y="16" width="480" height="480" rx="96" fill="#10153a"/>
    {stars}
    <!-- the monitor behind the eye -->
    <rect x="44" y="70" width="424" height="302" rx="20" fill="#d9dde3"/>
    <rect x="60" y="86" width="392" height="262" rx="8" fill="url(#screen)"/>
    <path d="M226,372 L214,424 L298,424 L286,372 Z" fill="#c3c8d0"/>
    <rect x="174" y="418" width="164" height="16" rx="8" fill="#d9dde3"/>
    <!-- the eye -->
    <path d="{EYE}" fill="#f7f4ec"/>
    <g clip-path="url(#eye)">
      <circle cx="{CX}" cy="{CY}" r="{R}" fill="url(#sea)"/>
      <g clip-path="url(#iris)">
        <path d="{graticule()}" fill="none" stroke="{grid}" stroke-width="0.9" opacity=".35"/>
        <path d="{' '.join(land)}" fill="{landc}" fill-rule="nonzero"/>
        <path d="{' '.join(uk)}" fill="{landc}" stroke="#ffd36b" stroke-width="{1.2 if ZOOM > 1 else 1.6}" stroke-linejoin="round"/>
      </g>
      <circle cx="{CX}" cy="{CY}" r="{R}" fill="none" stroke="#0e2a44" stroke-width="5"/>
    </g>
    <path d="{EYE}" fill="none" stroke="#0b0f2c" stroke-width="11" stroke-linejoin="round"/>
    <ellipse cx="{CX-62}" cy="{CY-14}" rx="15" ry="12" fill="#ffffff" opacity=".85"/>
    {sparkle(440, 104, 30, "#ffd36b")}{sparkle(84, 404, 18, "#ffd36b", .85)}
    </svg>
    '''
open(sys.argv[1] if len(sys.argv) > 1 else "F-eye-monitor.svg", "w").write(svg)
print(len(land), "land shapes,", len(uk), "UK shapes,", len(svg) // 1024, "KB")
