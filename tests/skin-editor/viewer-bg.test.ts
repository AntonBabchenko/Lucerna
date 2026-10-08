import { describe, expect, it } from 'vitest';
import {
  BG_LABEL,
  loadViewerBg,
  SWATCH_CSS,
  saveViewerBg,
  VIEWER_BGS,
  VIEWPORT_CSS,
} from '$lib/accounts/skin-editor/viewer-bg';

const KEY = 'lucerna.skinEditorBackground';

function memoryStorage(initial: Record<string, string> = {}): Storage {
  const m = new Map(Object.entries(initial));
  return {
    get length() {
      return m.size;
    },
    clear: () => m.clear(),
    getItem: (k) => m.get(k) ?? null,
    key: (i) => [...m.keys()][i] ?? null,
    removeItem: (k) => {
      m.delete(k);
    },
    setItem: (k, v) => {
      m.set(k, String(v));
    },
  };
}

const throwing: Storage = {
  length: 0,
  clear() {},
  key: () => null,
  getItem() {
    throw new DOMException('denied', 'SecurityError');
  },
  removeItem() {},
  setItem() {
    throw new DOMException('quota', 'QuotaExceededError');
  },
};

describe('viewer backgrounds', () => {
  it('are the neutral grey and three places, each named and drawn', () => {
    expect(VIEWER_BGS).toEqual(['neutral', 'plains', 'nether', 'stronghold']);
    for (const bg of VIEWER_BGS) {
      expect(BG_LABEL[bg]).toMatch(/^skinEditor\.bg/);
      expect(SWATCH_CSS[bg]).not.toBe('');
      expect(VIEWPORT_CSS[bg]).not.toBe('');
    }
  });
});

describe('the remembered background', () => {
  it('is neutral the first time', () => {
    expect(loadViewerBg(memoryStorage())).toBe('neutral');
  });

  it('comes back as it was saved', () => {
    const s = memoryStorage();
    saveViewerBg('nether', s);
    expect(s.getItem(KEY)).toBe('nether');
    expect(loadViewerBg(s)).toBe('nether');
  });

  // A value this version does not know (an older build's "dark", a hand-edited one) is not a
  // background: it falls back to the default rather than reaching the viewport.
  it.each(['dark', 'mid', '', '{"x":1}', 'NETHER'])('an unknown value %j reads as neutral', (v) => {
    expect(loadViewerBg(memoryStorage({ [KEY]: v }))).toBe('neutral');
  });

  it('unreadable storage reads as neutral, and a failed write does not throw', () => {
    expect(loadViewerBg(throwing)).toBe('neutral');
    expect(() => saveViewerBg('plains', throwing)).not.toThrow();
  });
});
