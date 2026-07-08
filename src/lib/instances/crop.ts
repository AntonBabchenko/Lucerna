export interface CropView {
  imgW: number;
  imgH: number;
  /** Display scale: frame px per image px (> 0). */
  scale: number;
  /** Scaled-image top-left X within the frame (px, <= 0 once clamped). */
  offsetX: number;
  offsetY: number;
  /** Square frame edge (px). */
  frame: number;
}

export interface CropRect {
  sx: number;
  sy: number;
  sSize: number;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/** Map the visible square of a pan/zoom view to a source-pixel crop rect. */
export function computeCropRect(v: CropView): CropRect {
  const sSize = Math.min(v.frame / v.scale, v.imgW, v.imgH);
  const sx = clamp(-v.offsetX / v.scale, 0, Math.max(0, v.imgW - sSize));
  const sy = clamp(-v.offsetY / v.scale, 0, Math.max(0, v.imgH - sSize));
  return { sx, sy, sSize };
}
