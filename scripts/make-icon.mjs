import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons");
mkdirSync(root, { recursive: true });

function png(size) {
  const raw = Buffer.alloc(size * (size * 4 + 1));
  const cx = (size - 1) / 2;
  const radius = size * 0.42;
  const eye = size * 0.06;
  for (let y = 0; y < size; y++) {
    const row = y * (size * 4 + 1);
    raw[row] = 0;
    for (let x = 0; x < size; x++) {
      const dx = x - cx;
      const dy = y - cx + size * 0.02;
      const dist = Math.hypot(dx, dy);
      const edge = Math.max(0, Math.min(1, radius - dist));
      const alpha = dist < radius - 1 ? 255 : Math.round(edge * 255);
      const i = row + 1 + x * 4;
      const face = dist < radius;
      let r = 246, g = 196, b = 83;
      const rim = dist > radius - size * 0.045;
      if (rim) { r = 44; g = 36; b = 28; }
      const eyeY = cy(size) - size * 0.06;
      for (const eyeX of [cx - size * 0.16, cx + size * 0.16]) {
        if (Math.hypot(x - eyeX, y - eyeY) < eye) { r = 44; g = 36; b = 28; }
      }
      const mouth = y > cy(size) + size * 0.08 && y < cy(size) + size * 0.12
        && Math.abs(x - cx) < size * 0.12;
      if (mouth && face && !rim) { r = 44; g = 36; b = 28; }
      if (!face) { r = 0; g = 0; b = 0; }
      raw[i] = r;
      raw[i + 1] = g;
      raw[i + 2] = b;
      raw[i + 3] = face ? alpha : 0;
    }
  }
  return encodePng(size, size, raw);
}

function cy(size) {
  return (size - 1) / 2 + size * 0.02;
}

function encodePng(width, height, raw) {
  const signature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  const compressed = deflateSync(raw);
  return Buffer.concat([
    signature,
    chunk("IHDR", ihdr),
    chunk("IDAT", compressed),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function chunk(type, data) {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(data.length, 0);
  head.write(type, 4, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([Buffer.from(type), data])) >>> 0, 0);
  return Buffer.concat([head, data, crc]);
}

function crc32(buf) {
  let c = ~0;
  for (const byte of buf) {
    c ^= byte;
    for (let i = 0; i < 8; i++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c;
}

function ico(pngBuffer, size) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(1, 4);
  const entry = Buffer.alloc(16);
  entry[0] = size >= 256 ? 0 : size;
  entry[1] = size >= 256 ? 0 : size;
  entry.writeUInt16LE(1, 4);
  entry.writeUInt16LE(32, 6);
  entry.writeUInt32LE(pngBuffer.length, 8);
  entry.writeUInt32LE(22, 12);
  return Buffer.concat([header, entry, pngBuffer]);
}

const sizes = {
  "32x32.png": 32,
  "128x128.png": 128,
  "128x128@2x.png": 256,
  "source.png": 1024,
};
for (const [name, size] of Object.entries(sizes)) {
  writeFileSync(join(root, name), png(size));
}
writeFileSync(join(root, "icon.ico"), ico(png(256), 256));
console.log("wrote png icons");
