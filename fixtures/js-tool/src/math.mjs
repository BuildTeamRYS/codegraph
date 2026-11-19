/**
 * Clamp `value` into the inclusive range [lo, hi].
 */
export function clamp(value, lo, hi) {
  return Math.min(Math.max(value, lo), hi);
}

export const clampFast = (v, lo, hi) => Math.min(Math.max(v, lo), hi);
