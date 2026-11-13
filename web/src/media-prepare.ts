export class PreparationError extends Error {}
/** Header preflight precedes browser decode, preventing compressed dimension bombs. */
export function imageDimensions(bytes: Uint8Array): [number, number] {
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const ascii = (at: number, n: number) =>
    String.fromCharCode(...bytes.slice(at, at + n));
  if (bytes.length >= 24 && bytes[0] === 137 && ascii(1, 3) === "PNG")
    return [v.getUint32(16), v.getUint32(20)];
  if (bytes.length >= 30 && ascii(0, 4) === "RIFF" && ascii(8, 4) === "WEBP") {
    const kind = ascii(12, 4);
    if (kind === "VP8X")
      return [
        1 + bytes[24] + (bytes[25] << 8) + (bytes[26] << 16),
        1 + bytes[27] + (bytes[28] << 8) + (bytes[29] << 16),
      ];
    if (
      kind === "VP8 " &&
      bytes[23] === 157 &&
      bytes[24] === 1 &&
      bytes[25] === 42
    )
      return [v.getUint16(26, true) & 16383, v.getUint16(28, true) & 16383];
    if (kind === "VP8L" && bytes[20] === 47) {
      const bits = v.getUint32(21, true);
      return [(bits & 16383) + 1, ((bits >>> 14) & 16383) + 1];
    }
  }
  if (bytes.length > 4 && bytes[0] === 255 && bytes[1] === 216) {
    let at = 2;
    while (at + 4 <= bytes.length) {
      if (bytes[at++] !== 255) break;
      while (bytes[at] === 255) at++;
      const mark = bytes[at++];
      if (mark === 217 || mark === 218) break;
      if (mark === 1 || (mark >= 208 && mark <= 215)) continue;
      if (at + 2 > bytes.length) break;
      const size = v.getUint16(at);
      if (size < 2 || at + size > bytes.length) break;
      if (
        [
          192, 193, 194, 195, 197, 198, 199, 201, 202, 203, 205, 206, 207,
        ].includes(mark) &&
        size >= 7
      )
        return [v.getUint16(at + 5), v.getUint16(at + 3)];
      at += size;
    }
  }
  throw new PreparationError(
    "PNG, JPEG, WebP 이미지만 지원합니다. PDF·오디오·동영상 분석은 아직 지원하지 않습니다.",
  );
}
export async function prepareImage(file: File): Promise<Blob> {
  if (typeof createImageBitmap !== "function")
    throw new PreparationError(
      "이 브라우저에서 이미지 준비를 지원하지 않습니다. 최신 브라우저를 사용하세요.",
    );
  if (file.size > 32 * 1024 * 1024)
    throw new PreparationError("원본 이미지는 32MiB 이하여야 합니다.");
  const [width, height] = imageDimensions(
    new Uint8Array(await file.arrayBuffer()),
  );
  if (!width || !height || width * height > 48_000_000)
    throw new PreparationError("원본 이미지는 48MP 이하여야 합니다.");
  const bitmap = await createImageBitmap(file, {
    imageOrientation: "from-image",
  });
  try {
    if (bitmap.width * bitmap.height > 48_000_000)
      throw new PreparationError("원본 이미지는 48MP 이하여야 합니다.");
    let scale = Math.min(1, 2400 / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement("canvas");
    for (let pass = 0; pass < 5; pass++) {
      canvas.width = Math.max(1, Math.round(bitmap.width * scale));
      canvas.height = Math.max(1, Math.round(bitmap.height * scale));
      const ctx = canvas.getContext("2d");
      if (!ctx)
        throw new PreparationError(
          "이 브라우저에서 이미지 준비를 지원하지 않습니다.",
        );
      ctx.fillStyle = "#fff";
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      for (const quality of [0.88, 0.72, 0.55, 0.38]) {
        const blob = await new Promise<Blob | null>((resolve) =>
          canvas.toBlob(resolve, "image/jpeg", quality),
        );
        if (blob && blob.type === "image/jpeg" && blob.size <= 2 * 1024 * 1024)
          return blob;
      }
      scale *= 0.75;
    }
    throw new PreparationError(
      "이미지를 2MiB 이하로 준비하지 못했습니다. 작은 이미지를 선택하세요.",
    );
  } finally {
    bitmap.close();
  }
}
