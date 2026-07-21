// Pure roving-tabindex key handling, extracted so the arrow-key math is
// unit-testable and shared across TabBar / SegmentedControl / ToggleChipGroup
// (mirrors select-placement.ts: pure logic out, DOM wiring stays in the
// component). Each caller keeps its own bind:this refs, tabindex, and onChange —
// this only answers "given a key + current index, what is the next index?".

export type RovingOrientation = 'horizontal' | 'vertical' | 'both';

/**
 * Next focus index for a roving group, or null when the key is not a navigation
 * key (the caller should ignore it and let the event through). Arrow keys wrap
 * around the ends; Home/End jump to first/last. `horizontal` handles
 * ArrowLeft/Right, `vertical` ArrowUp/Down, `both` handles all four.
 *
 * Returns null (no-op) when the group is empty or has no current selection, so
 * a caller that can't resolve `current` simply does nothing — matching the
 * `if (current === -1) return;` guard the components used before extraction.
 */
export function nextRovingIndex(
  key: string,
  current: number,
  count: number,
  orientation: RovingOrientation = 'horizontal',
): number | null {
  if (count <= 0 || current < 0 || current >= count) return null;
  const horizontal = orientation === 'horizontal' || orientation === 'both';
  const vertical = orientation === 'vertical' || orientation === 'both';
  const forward = (horizontal && key === 'ArrowRight') || (vertical && key === 'ArrowDown');
  const backward = (horizontal && key === 'ArrowLeft') || (vertical && key === 'ArrowUp');
  if (forward) return (current + 1) % count;
  if (backward) return (current - 1 + count) % count;
  if (key === 'Home') return 0;
  if (key === 'End') return count - 1;
  return null;
}
