import { describe, expect, it } from 'vitest';
import { nextRovingIndex } from '$lib/ui/roving';

describe('nextRovingIndex', () => {
  it('horizontal: ArrowRight advances and wraps at the end', () => {
    expect(nextRovingIndex('ArrowRight', 0, 3, 'horizontal')).toBe(1);
    expect(nextRovingIndex('ArrowRight', 2, 3, 'horizontal')).toBe(0);
  });

  it('horizontal: ArrowLeft retreats and wraps at the start', () => {
    expect(nextRovingIndex('ArrowLeft', 2, 3, 'horizontal')).toBe(1);
    expect(nextRovingIndex('ArrowLeft', 0, 3, 'horizontal')).toBe(2);
  });

  it('Home/End jump to the ends regardless of orientation', () => {
    expect(nextRovingIndex('Home', 2, 4, 'horizontal')).toBe(0);
    expect(nextRovingIndex('End', 0, 4, 'vertical')).toBe(3);
    expect(nextRovingIndex('Home', 3, 4, 'both')).toBe(0);
  });

  it('horizontal ignores vertical arrows and vice versa', () => {
    expect(nextRovingIndex('ArrowDown', 0, 3, 'horizontal')).toBeNull();
    expect(nextRovingIndex('ArrowUp', 1, 3, 'horizontal')).toBeNull();
    expect(nextRovingIndex('ArrowRight', 0, 3, 'vertical')).toBeNull();
    expect(nextRovingIndex('ArrowLeft', 1, 3, 'vertical')).toBeNull();
  });

  it('vertical: ArrowDown/Up wrap', () => {
    expect(nextRovingIndex('ArrowDown', 2, 3, 'vertical')).toBe(0);
    expect(nextRovingIndex('ArrowUp', 0, 3, 'vertical')).toBe(2);
  });

  it('both: all four arrows navigate', () => {
    expect(nextRovingIndex('ArrowRight', 0, 3, 'both')).toBe(1);
    expect(nextRovingIndex('ArrowDown', 0, 3, 'both')).toBe(1);
    expect(nextRovingIndex('ArrowLeft', 0, 3, 'both')).toBe(2);
    expect(nextRovingIndex('ArrowUp', 0, 3, 'both')).toBe(2);
  });

  it('returns null for non-navigation keys', () => {
    expect(nextRovingIndex('Enter', 0, 3, 'both')).toBeNull();
    expect(nextRovingIndex('a', 0, 3, 'horizontal')).toBeNull();
    expect(nextRovingIndex(' ', 0, 3, 'both')).toBeNull();
  });

  it('is a no-op when the group is empty or has no current selection', () => {
    expect(nextRovingIndex('ArrowRight', -1, 3, 'horizontal')).toBeNull();
    expect(nextRovingIndex('ArrowRight', 0, 0, 'horizontal')).toBeNull();
    expect(nextRovingIndex('Home', 5, 3, 'horizontal')).toBeNull();
  });
});
