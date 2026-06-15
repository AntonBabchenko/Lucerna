import { beforeEach, describe, expect, it } from 'vitest';
import { loadRainbowEnabled, rainbowFx } from '$lib/fx/rainbow-fx.svelte';

const KEY = 'lucerna.fx.rainbowIcons';

beforeEach(() => {
  localStorage.clear();
});

describe('rainbow-fx', () => {
  it('defaults to enabled when nothing is stored', () => {
    expect(loadRainbowEnabled()).toBe(true);
  });

  it('reads a stored false value', () => {
    localStorage.setItem(KEY, 'false');
    expect(loadRainbowEnabled()).toBe(false);
  });

  it('reads a stored true value', () => {
    localStorage.setItem(KEY, 'true');
    expect(loadRainbowEnabled()).toBe(true);
  });

  it('set() updates state and persists to localStorage', () => {
    rainbowFx.set(false);
    expect(rainbowFx.enabled).toBe(false);
    expect(localStorage.getItem(KEY)).toBe('false');
    rainbowFx.set(true);
    expect(rainbowFx.enabled).toBe(true);
    expect(localStorage.getItem(KEY)).toBe('true');
  });
});
