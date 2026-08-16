// Minecraft skins are 64×64 (modern) or 64×32 (legacy). The head is at
// (8,8)–(16,16); the hat/overlay layer at (40,8)–(48,16) — both present in
// either layout. We draw face then hat on top, pixelated (no smoothing).
const FACE = { sx: 8, sy: 8, sw: 8, sh: 8 } as const;
const HAT = { sx: 40, sy: 8, sw: 8, sh: 8 } as const;

/**
 * Draw the player's head (face + hat overlay) onto a square canvas.
 *
 * Returns a dispose fn that aborts a still-pending image decode, so a stale
 * load (e.g. after the account switched) cannot paint a canvas that has since
 * been torn down — this also prevents the decoded Image from being retained.
 * `onError` fires if the PNG fails to decode, letting the caller fall back to
 * the letter avatar instead of showing a blank canvas.
 */
export function drawHead(
  canvas: HTMLCanvasElement,
  pngBase64: string,
  size: number,
  onError?: () => void,
): () => void {
  let aborted = false;
  const ctx = canvas.getContext('2d');
  if (!ctx) return () => {};
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.round(size * dpr);
  canvas.height = Math.round(size * dpr);
  canvas.style.width = `${size}px`;
  canvas.style.height = `${size}px`;
  const img = new Image(); // no-network-ok: new Image( decodes a local data: URL
  img.onload = () => {
    if (aborted) return;
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(img, FACE.sx, FACE.sy, FACE.sw, FACE.sh, 0, 0, canvas.width, canvas.height);
    ctx.drawImage(img, HAT.sx, HAT.sy, HAT.sw, HAT.sh, 0, 0, canvas.width, canvas.height);
  };
  img.onerror = () => {
    if (!aborted) onError?.();
  };
  img.src = `data:image/png;base64,${pngBase64}`;
  return () => {
    aborted = true;
  };
}
