// Generates a tech-style FleetWatch app icon (1024x1024 RGBA PNG), dependency-free.
// Concept: dark squircle · radar concentric rings · sweep wedge · fleet node network · neon glow.
// Run:  node scripts/gen-icon.mjs   ->   icon-tech.png   (then: tauri icon icon-tech.png)
import zlib from "zlib";
import fs from "fs";

const N = 1024;
const cx = N / 2, cy = N / 2;
const TWO = Math.PI * 2;
const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);
const lerp = (a, b, t) => a + (b - a) * t;

// signed distance to a centered rounded rect (negative = inside)
function sdRoundRect(x, y, r) {
  const dx = Math.abs(x - cx) - (N / 2 - r);
  const dy = Math.abs(y - cy) - (N / 2 - r);
  const out = Math.hypot(Math.max(dx, 0), Math.max(dy, 0)) - r;
  const ins = Math.min(Math.max(dx, dy), 0);
  return out + ins;
}
function distSeg(px, py, ax, ay, bx, by) {
  const dx = bx - ax, dy = by - ay;
  const l2 = dx * dx + dy * dy;
  const t = l2 ? clamp(((px - ax) * dx + (py - ay) * dy) / l2, 0, 1) : 0;
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}
const pol = (a, r) => [cx + Math.cos(a) * r, cy + Math.sin(a) * r];

const cyan = [70, 226, 255];
const blue = [70, 140, 248];
const white = [205, 248, 255];

const lead = -Math.PI / 4; // radar sweep "leading edge" angle
const rings = [118, 208, 300, 392];
const nodes = [
  pol(lead, 300),
  pol(lead + TWO * 0.3, 208),
  pol(lead + TWO * 0.56, 300),
  pol(lead + TWO * 0.8, 150),
];

const data = Buffer.alloc(N * N * 4);

for (let y = 0; y < N; y++) {
  for (let x = 0; x < N; x++) {
    const sd = sdRoundRect(x, y, 232);
    const cov = clamp(0.5 - sd, 0, 1);
    const i = (y * N + x) * 4;
    if (cov <= 0) {
      data[i + 3] = 0;
      continue;
    }
    const rr = Math.hypot(x - cx, y - cy);
    const ang = Math.atan2(y - cy, x - cx);
    const da = (((ang - lead) % TWO) + TWO) % TWO; // 0..2pi trailing from the sweep edge

    // background: vertical gradient + radial vignette
    const ty = y / N;
    const vig = clamp(1 - (rr / (N * 0.72)) ** 2 * 0.95, 0, 1);
    let r = lerp(13, 4, ty) * vig;
    let g = lerp(20, 8, ty) * vig;
    let b = lerp(38, 16, ty) * vig;

    // faint grid
    if (x % 64 < 1.3 || y % 64 < 1.3) {
      r += cyan[0] * 0.045; g += cyan[1] * 0.045; b += cyan[2] * 0.045;
    }

    // sweep wedge soft fill (brightest just behind the leading edge)
    if (rr < 405) {
      const w = Math.exp(-(da * da) / (2 * 0.6 * 0.6)) * Math.exp(-(rr * rr) / (2 * 270 * 270)) * 0.55;
      r += blue[0] * w; g += blue[1] * w; b += blue[2] * w;
    }

    // radar rings, modulated by the sweep
    for (const R of rings) {
      const dr = rr - R;
      const k = Math.exp(-(dr * dr) / (2 * 7 * 7));
      if (k < 0.004) continue;
      const sweep = 0.22 + 0.85 * Math.exp(-(da * da) / (2 * 0.95 * 0.95));
      const inten = k * sweep * 0.95;
      r += cyan[0] * inten; g += cyan[1] * inten; b += cyan[2] * inten;
    }

    // node network: connecting lines + glow + bright cores
    for (const [nx, ny] of nodes) {
      const dl = distSeg(x, y, cx, cy, nx, ny);
      const lg = Math.exp(-(dl * dl) / (2 * 2.4 * 2.4)) * 0.45;
      r += cyan[0] * lg; g += cyan[1] * lg; b += cyan[2] * lg;
      const dn = Math.hypot(x - nx, y - ny);
      const glow = Math.exp(-(dn * dn) / (2 * 26 * 26)) * 0.9;
      r += cyan[0] * glow; g += cyan[1] * glow; b += cyan[2] * glow;
      const core = clamp(0.5 - (dn - 13), 0, 1);
      r = lerp(r, white[0], core); g = lerp(g, white[1], core); b = lerp(b, white[2], core);
    }

    // center node (the "watcher")
    const cg = Math.exp(-(rr * rr) / (2 * 64 * 64));
    r += cyan[0] * cg; g += cyan[1] * cg; b += cyan[2] * cg;
    const ccore = clamp(0.5 - (rr - 22), 0, 1);
    r = lerp(r, white[0], ccore); g = lerp(g, white[1], ccore); b = lerp(b, white[2], ccore);

    // crisp inner rim
    const rim = Math.exp(-(((sd + 6) * (sd + 6)) / (2 * 2.2 * 2.2))) * 0.5;
    r += cyan[0] * rim; g += cyan[1] * rim; b += cyan[2] * rim;

    data[i] = clamp(r, 0, 255);
    data[i + 1] = clamp(g, 0, 255);
    data[i + 2] = clamp(b, 0, 255);
    data[i + 3] = 255 * cov;
  }
}

// ── minimal PNG encoder (RGBA, color type 6) ──────────────────────────────────
function crc32(buf) {
  let c = ~0 >>> 0;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return (~c) >>> 0;
}
function chunk(type, body) {
  const len = Buffer.alloc(4); len.writeUInt32BE(body.length, 0);
  const t = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(Buffer.concat([t, body])), 0);
  return Buffer.concat([len, t, body, crc]);
}
const raw = Buffer.alloc((N * 4 + 1) * N);
let o = 0;
for (let y = 0; y < N; y++) {
  raw[o++] = 0; // no filter
  data.copy(raw, o, y * N * 4, y * N * 4 + N * 4);
  o += N * 4;
}
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(N, 0); ihdr.writeUInt32BE(N, 4);
ihdr[8] = 8; ihdr[9] = 6; // 8-bit, RGBA
const sig = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const png = Buffer.concat([
  sig,
  chunk("IHDR", ihdr),
  chunk("IDAT", zlib.deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);
fs.writeFileSync(new URL("../icon-tech.png", import.meta.url), png);
console.log("wrote icon-tech.png", png.length, "bytes");
