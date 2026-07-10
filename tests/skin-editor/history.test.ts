import { describe, expect, it } from 'vitest';
import { SkinHistory } from '$lib/accounts/skin-editor/history';

const buf = (v: number) => new Uint8ClampedArray([v]);

describe('SkinHistory', () => {
  it('undo returns the pre-stroke state and redo re-applies', () => {
    const h = new SkinHistory(10);
    h.begin(buf(0)); // snapshot state 0 before painting -> state becomes 1
    expect(h.canUndo).toBe(true);
    const undone = h.undo(buf(1));
    expect(undone).not.toBeNull();
    expect([...(undone as Uint8ClampedArray)]).toEqual([0]);
    const redone = h.redo(buf(0));
    expect([...(redone as Uint8ClampedArray)]).toEqual([1]);
  });

  it('returns null when nothing to undo/redo', () => {
    const h = new SkinHistory(10);
    expect(h.undo(buf(5))).toBeNull();
    expect(h.redo(buf(5))).toBeNull();
  });

  it('a new stroke clears the redo stack', () => {
    const h = new SkinHistory(10);
    h.begin(buf(0));
    h.undo(buf(1)); // redo now holds [1]
    expect(h.canRedo).toBe(true);
    h.begin(buf(0)); // new stroke
    expect(h.canRedo).toBe(false);
  });

  it('caps the undo depth by dropping the oldest snapshot', () => {
    const h = new SkinHistory(2);
    h.begin(buf(0));
    h.begin(buf(1));
    h.begin(buf(2));
    expect(h.depth).toBe(2);
    // deepest reachable state is 1 (0 was dropped)
    const first = h.undo(buf(3));
    expect([...(first as Uint8ClampedArray)]).toEqual([2]);
    const second = h.undo(buf(2));
    expect([...(second as Uint8ClampedArray)]).toEqual([1]);
    expect(h.undo(buf(1))).toBeNull();
  });

  it('snapshots are copies — later mutation of the source does not corrupt history', () => {
    const h = new SkinHistory(10);
    const live = buf(7);
    h.begin(live);
    live[0] = 99; // simulate the stroke mutating the live buffer
    const undone = h.undo(live);
    expect([...(undone as Uint8ClampedArray)]).toEqual([7]);
  });
});
