// Generates the Erindi logo: three normal-distribution peaks in the aurora colors.
// Writes src/logo.svg for the UI and app-icon.png (1024 px) as the source for `pnpm tauri icon`.
import { writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";

const SIZE = 1024;
const RADIUS = 224;
const BACKGROUND = [10, 10, 20];
// One color per peak, left to right: the aurora colors of the pipeline stages.
const STOPS = [
  [0.25, [34, 197, 94]], // green
  [0.5, [245, 158, 11]], // orange
  [0.75, [59, 130, 246]], // blue
];
const PEAKS = [
  [0.25, 0.62],
  [0.5, 1],
  [0.75, 0.62],
];
const SIGMA = 0.062;
const LEFT = 0;
const RIGHT = 1;
const BASELINE = 0.72;
const AMPLITUDE = 0.46;
const STROKE = 44;

const gauss = (x, c) => Math.exp(-((x - c) ** 2) / (2 * SIGMA ** 2));
/** Wave height in 0..1 over the icon width fraction u in 0..1. */
const wave = (u) => PEAKS.reduce((sum, [c, h]) => sum + h * gauss(u, c), 0);
const peak = Math.max(...Array.from({ length: 1001 }, (_, i) => wave(i / 1000)));
/** Curve y in pixels for pixel x, or null outside the wave span. */
const curveY = (x) => {
  const u = (x / SIZE - LEFT) / (RIGHT - LEFT);
  if (u < 0 || u > 1) return null;
  return SIZE * (BASELINE - (AMPLITUDE * wave(u)) / peak);
};
const color = (x) => {
  const t = Math.min(1, Math.max(0, (x / SIZE - LEFT) / (RIGHT - LEFT)));
  const i = t <= STOPS[1][0] ? 0 : 1;
  const [t0, a] = STOPS[i];
  const [t1, b] = STOPS[i + 1];
  const k = Math.min(1, Math.max(0, (t - t0) / (t1 - t0)));
  return a.map((v, j) => v + (b[j] - v) * k);
};
const insideTile = (x, y) => {
  const cx = Math.min(Math.max(x, RADIUS), SIZE - RADIUS);
  const cy = Math.min(Math.max(y, RADIUS), SIZE - RADIUS);
  return (x - cx) ** 2 + (y - cy) ** 2 <= RADIUS ** 2;
};

// PNG: 2x2 supersampling for smooth edges.
const SS = 2;
/** Shortest distance from (x, y) to the curve, searched a little to each side. */
const distance = (x, y) => {
  let best = Infinity;
  for (let dx = -60; dx <= 60; dx += 2) {
    const cy = curveY(x + dx);
    if (cy !== null) best = Math.min(best, Math.hypot(dx, y - cy));
  }
  return best;
};
const raw = Buffer.alloc((SIZE * 4 + 1) * SIZE);
for (let py = 0; py < SIZE; py++) {
  raw[py * (SIZE * 4 + 1)] = 0;
  for (let px = 0; px < SIZE; px++) {
    let r = 0, g = 0, b = 0, a = 0;
    for (let sy = 0; sy < SS; sy++) {
      for (let sx = 0; sx < SS; sx++) {
        const x = px + (sx + 0.5) / SS;
        const y = py + (sy + 0.5) / SS;
        if (!insideTile(x, y)) continue;
        let pixel = [...BACKGROUND];
        const cy = curveY(x);
        if (cy !== null) {
          const c = color(x);
          const dist = Math.abs(y - cy) < 90 ? distance(x, y) : Infinity;
          // Soft glow around the line, fading fill below it, crisp line on top.
          const glow = 0.4 * Math.exp(-((dist / 34) ** 2));
          const fill = y > cy ? 0.5 * Math.max(0, 1 - (y - cy) / (SIZE - cy)) : 0;
          const line = dist < STROKE / 2 ? 1 : 0;
          const mix = Math.min(1, Math.max(glow, fill, line));
          const tint = line ? c.map((v) => v + (255 - v) * 0.15) : c;
          pixel = pixel.map((v, j) => v + (tint[j] - v) * mix);
        }
        r += pixel[0];
        g += pixel[1];
        b += pixel[2];
        a += 255;
      }
    }
    const n = SS * SS;
    const i = py * (SIZE * 4 + 1) + 1 + px * 4;
    const cov = a / n / 255;
    raw[i] = cov ? r / (n * cov) : 0;
    raw[i + 1] = cov ? g / (n * cov) : 0;
    raw[i + 2] = cov ? b / (n * cov) : 0;
    raw[i + 3] = a / n;
  }
}

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc = (buf) => {
  let c = 0xffffffff;
  for (const v of buf) c = crcTable[(c ^ v) & 255] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type), data]);
  const sum = Buffer.alloc(4);
  sum.writeUInt32BE(crc(body));
  return Buffer.concat([len, body, sum]);
};
const header = Buffer.alloc(13);
header.writeUInt32BE(SIZE, 0);
header.writeUInt32BE(SIZE, 4);
header[8] = 8;
header[9] = 6;
writeFileSync(
  "app-icon.png",
  Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]),
);

// SVG: the same curve as a path.
const points = [];
for (let x = LEFT * SIZE; x <= RIGHT * SIZE; x += 8) points.push([x, curveY(x)]);
const line = points.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)} ${y.toFixed(1)}`).join(" ");
const area = `${line} L${(RIGHT * SIZE).toFixed(1)} ${SIZE} L${(LEFT * SIZE).toFixed(1)} ${SIZE} Z`;
const rgb = (c) => `rgb(${c.map(Math.round).join(",")})`;
const stops = STOPS.map(([t, c]) => `<stop offset="${t}" stop-color="${rgb(c)}"/>`).join("");
const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${SIZE} ${SIZE}" role="img" aria-label="Erindi">
<defs>
<linearGradient id="h" x1="${LEFT * SIZE}" x2="${RIGHT * SIZE}" y1="0" y2="0" gradientUnits="userSpaceOnUse">${stops}</linearGradient>
<linearGradient id="v" x1="0" x2="0" y1="${SIZE * (BASELINE - AMPLITUDE)}" y2="${SIZE}" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#fff" stop-opacity="0.5"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
<mask id="fade"><rect width="${SIZE}" height="${SIZE}" fill="url(#v)"/></mask>
<filter id="glow" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="22"/></filter>
</defs>
<clipPath id="tile"><rect width="${SIZE}" height="${SIZE}" rx="${RADIUS}"/></clipPath>
<rect width="${SIZE}" height="${SIZE}" rx="${RADIUS}" fill="${rgb(BACKGROUND)}"/>
<g clip-path="url(#tile)">
<path d="${area}" fill="url(#h)" mask="url(#fade)"/>
<path d="${line}" fill="none" stroke="url(#h)" stroke-width="${STROKE * 1.8}" stroke-linecap="round" opacity="0.45" filter="url(#glow)"/>
<path d="${line}" fill="none" stroke="url(#h)" stroke-width="${STROKE}" stroke-linecap="round"/>
</g>
</svg>
`;
writeFileSync("src/logo.svg", svg);
console.log("wrote app-icon.png and src/logo.svg");
