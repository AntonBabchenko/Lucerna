import { beforeEach, describe, expect, it } from 'vitest';
import { iconZoomFx, loadIconZoomEnabled } from '$lib/fx/icon-zoom-fx.svelte';

const KEY = 'lucerna.fx.iconZoom';

beforeEach(() => {
  localStorage.clear();
});

describe('icon-zoom-fx', () => {
  it('defaults to enabled when nothing is stored', () => {
    expect(loadIconZoomEnabled()).toBe(true);
  });

  it('reads a stored false value', () => {
    localStorage.setItem(KEY, 'false');
    expect(loadIconZoomEnabled()).toBe(false);
  });

  it('reads a stored true value', () => {
    localStorage.setItem(KEY, 'true');
    expect(loadIconZoomEnabled()).toBe(true);
  });

  it('set() updates state and persists to localStorage', () => {
    iconZoomFx.set(false);
    expect(iconZoomFx.enabled).toBe(false);
    expect(localStorage.getItem(KEY)).toBe('false');
    iconZoomFx.set(true);
    expect(iconZoomFx.enabled).toBe(true);
    expect(localStorage.getItem(KEY)).toBe('true');
  });
});
