// Generates a 1024×1024 PNG app icon for Research Core — a rounded "RC" mark
// on the accent-blue background, matching the design's brand chip.
// Pure Node (zlib), no native deps.
import zlib from "node:zlib";
import fs from "node:fs";
import path from "node:path";

const W = 1024, H = 1024;
const ACCENT = [0x3B, 0x5B, 0xDB];
const WHITE = [0xFF, 0xFF, 0xFF];

// raw RGBA
const raw = Buffer.alloc(W * H * 4);
function setPx(x, y, [r, g, b], a = 255) {
  const i = (y * W + x) * 4;
  raw[i] = r; raw[i + 1] = g; raw[i + 2] = b; raw[i + 3] = a;
}

// rounded-rect mask (full bleed, ~18% radius)
const r = 230;
function insideRounded(x, y) {
  // treat corners
  const rx = Math.min(x, W - 1 - x);
  const ry = Math.min(y, H - 1 - y);
  if (rx >= r || ry >= r) return true;
  const dx = r - rx, dy = r - ry;
  return dx * dx + dy * dy <= r * r;
}

// crude block "RC" glyph bitmap — draw two letters with rectangles
const glyph = [
  // R
  [220, 280, 90, 464],  // vertical stem
  [220, 280, 376, 326], // top bar
  [220, 376, 376, 326], // mid bar
  [326, 280, 376, 376], // right top down to mid (bowel top)
  [326, 376, 410, 426], // belly right
  [376, 426, 410, 376], // belly bottom (angled approx)
  [376, 376, 460, 464], // diagonal leg
  // C
  [560, 280, 636, 326], // top
  [560, 280, 604, 464], // left stem
  [560, 418, 636, 464], // bottom
];
function inGlyph(x, y) {
  for (const [x0, y0, x1, y1] of glyph) {
    if (x >= x0 && x < x1 && y >= y0 && y < y1) return true;
  }
  return false;
}

for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    if (!insideRounded(x, y)) { setPx(x, y, [0, 0, 0], 0); continue; }
    setPx(x, y, inGlyph(x, y) ? WHITE : ACCENT);
  }
}

// PNG encode
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])), 0);
  return Buffer.concat([len, typeBuf, data, crc]);
}
const crcTable = (() => {
  const t = []; for (let n = 0; n < 256; n++) {
    let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  } return t;
})();
function crc32(buf) {
  let c = 0xFFFFFFFF; for (const b of buf) c = crcTable[(c ^ b) & 0xFF] ^ (c >>> 8);
  return (c ^ 0xFFFFFFFF) >>> 0;
}
// add filter byte (0) per scanline
const filtered = Buffer.alloc((W * 4 + 1) * H);
for (let y = 0; y < H; y++) {
  filtered[y * (W * 4 + 1)] = 0;
  raw.copy(filtered, y * (W * 4 + 1) + 1, y * W * 4, (y + 1) * W * 4);
}
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(W, 0); ihdr.writeUInt32BE(H, 4);
ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
const png = Buffer.concat([
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
  chunk("IHDR", ihdr),
  chunk("IDAT", zlib.deflateSync(filtered)),
  chunk("IEND", Buffer.alloc(0)),
]);
const outDir = path.join(process.cwd(), "src-tauri", "icons");
fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "icon.png"), png);
fs.writeFileSync(path.join(outDir, "icon-source.png"), png);
console.log("wrote icon.png", png.length, "bytes");
