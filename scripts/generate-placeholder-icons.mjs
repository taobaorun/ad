#!/usr/bin/env node
// Generates placeholder solid-color PNGs at the sizes Tauri expects.
// Replace with `pnpm tauri icon <real-source.png>` once you have a real icon.
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import zlib from 'node:zlib';

const COLOR = [0x7c, 0x3a, 0xed]; // brand purple

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, 'ascii');
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(zlib.crc32(Buffer.concat([typeBuf, data])) >>> 0, 0);
  return Buffer.concat([len, typeBuf, data, crc]);
}

function makePng(w, h) {
  const stride = w * 4 + 1;
  const raw = Buffer.alloc(stride * h);
  for (let y = 0; y < h; y++) {
    raw[y * stride] = 0; // filter: None
    for (let x = 0; x < w; x++) {
      const off = y * stride + 1 + x * 4;
      raw[off] = COLOR[0];
      raw[off + 1] = COLOR[1];
      raw[off + 2] = COLOR[2];
      raw[off + 3] = 255;
    }
  }
  const idat = zlib.deflateSync(raw);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;
  const sig = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  return Buffer.concat([
    sig,
    chunk('IHDR', ihdr),
    chunk('IDAT', idat),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

const targets = [
  ['src-tauri/icons/32x32.png', 32, 32],
  ['src-tauri/icons/128x128.png', 128, 128],
  ['src-tauri/icons/128x128@2x.png', 256, 256],
];

for (const [path, w, h] of targets) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, makePng(w, h));
  console.log(`wrote ${path} (${w}×${h})`);
}
