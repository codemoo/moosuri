// Run with the web Vite development server and a desktop browser; no account or server API is used.
import { prepareImage } from "../src/media-prepare.ts";

const MiB = 1024 * 1024;
async function photo(width, bytes) {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = 6000;
  canvas.getContext("2d").fillRect(0, 0, width, 6000);
  const jpeg = await new Promise(resolve => canvas.toBlob(resolve, "image/jpeg"));
  canvas.width = canvas.height = 1;
  const pieces = [jpeg.slice(0, 2)];
  // Valid APP15 segments pad the fixture to the exact byte boundary without altering its pixels.
  let remaining = bytes - jpeg.size;
  while (remaining > 0) {
    const length = Math.min(remaining, 65537);
    if (length < 4) throw new Error("invalid fixture padding");
    pieces.push(Uint8Array.of(255, 239, (length - 2) >> 8, (length - 2) & 255), new Uint8Array(length - 4));
    remaining -= length;
  }
  pieces.push(jpeg.slice(2));
  return new File(pieces, "synthetic.jpg", {type: "image/jpeg"});
}

export async function verifyPhotoLimits() {
  const results = [];
  for (const [width, bytes, rejection] of [
    [7999, 32 * MiB - 1], [8000, 32 * MiB],
    [8001, 32 * MiB, "48MP"], [8000, 32 * MiB + 1, "32MiB"],
  ]) {
    const input = await photo(width, bytes);
    if (input.size !== bytes) throw new Error("fixture byte count");
    let output;
    try { output = await prepareImage(input); }
    catch (error) {
      if (!rejection || !error.message.includes(rejection)) throw error;
      results.push({width, bytes, rejected: rejection});
      continue;
    }
    if (rejection) throw new Error("out-of-range photo accepted");
    const bitmap = await createImageBitmap(output);
    const expectedHeight = Math.round(6000 * 2400 / width);
    try {
      if (output.type !== "image/jpeg" || output.size > 2 * MiB || bitmap.width !== 2400 || bitmap.height !== expectedHeight)
        throw new Error("invalid prepared photo");
      results.push({width, bytes, output: [bitmap.width, bitmap.height, output.size]});
    } finally { bitmap.close(); }
  }
  return results;
}
