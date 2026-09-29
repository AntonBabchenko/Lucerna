import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  __resetLayers,
  insertTour,
  isTopmost,
  modalDepth,
  newLayerId,
  onLayersChange,
  pushLayer,
  tourAbove,
} from '$lib/ui/layer-stack.svelte';

afterEach(() => __resetLayers());

const noop = () => {};

function pressEscape(): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
  window.dispatchEvent(e);
  return e;
}

describe('layer stack order', () => {
  it('the layer pushed last is on top; releasing it hands the top back', () => {
    const a = newLayerId('a');
    const b = newLayerId('b');
    pushLayer(a, 'modal', noop);
    const releaseB = pushLayer(b, 'popover', noop);
    expect(isTopmost(b)).toBe(true);
    expect(isTopmost(a)).toBe(false);
    releaseB();
    expect(isTopmost(a)).toBe(true);
  });

  it('release is idempotent and never removes another layer', () => {
    const a = newLayerId('a');
    const b = newLayerId('b');
    const releaseA = pushLayer(a, 'modal', noop);
    pushLayer(b, 'modal', noop);
    releaseA();
    releaseA();
    expect(isTopmost(b)).toBe(true);
    expect(modalDepth()).toBe(1);
  });

  it('modalDepth counts modals only', () => {
    pushLayer(newLayerId('m'), 'modal', noop);
    pushLayer(newLayerId('p'), 'popover', noop);
    insertTour(newLayerId('t'), null, noop);
    expect(modalDepth()).toBe(1);
  });
});

describe('insertTour', () => {
  it('puts a page tour at the bottom, under layers already open', () => {
    const m = newLayerId('m');
    const t = newLayerId('t');
    pushLayer(m, 'modal', noop);
    expect(insertTour(t, null, noop)).not.toBeNull();
    expect(isTopmost(m)).toBe(true);
    expect(isTopmost(t)).toBe(false);
  });

  it('puts a hosted tour directly above its host, under anything opened later', () => {
    const m = newLayerId('m');
    const t = newLayerId('t');
    const p = newLayerId('p');
    pushLayer(m, 'modal', noop);
    insertTour(t, m, noop);
    expect(isTopmost(t)).toBe(true);
    const releaseP = pushLayer(p, 'popover', noop);
    expect(isTopmost(t)).toBe(false);
    releaseP();
    expect(isTopmost(t)).toBe(true);
  });

  it('refuses a second tour on the same host', () => {
    expect(insertTour(newLayerId('t1'), null, noop)).not.toBeNull();
    expect(insertTour(newLayerId('t2'), null, noop)).toBeNull();
  });

  it('refuses a page tour while a tour runs above a modal', () => {
    const m = newLayerId('m');
    pushLayer(m, 'modal', noop);
    insertTour(newLayerId('t'), m, noop);
    expect(insertTour(newLayerId('page'), null, noop)).toBeNull();
  });

  it('nests a tour over a layer that lies above every running tour', () => {
    const page = newLayerId('page');
    const m = newLayerId('m');
    const nested = newLayerId('nested');
    insertTour(page, null, noop);
    pushLayer(m, 'modal', noop);
    expect(insertTour(nested, m, noop)).not.toBeNull();
    expect(isTopmost(nested)).toBe(true);
  });

  it('refuses a host that is not in the stack', () => {
    expect(insertTour(newLayerId('t'), newLayerId('gone'), noop)).toBeNull();
  });

  it('releasing a host drops the tours it hosts', () => {
    const m = newLayerId('m');
    const t = newLayerId('t');
    const releaseM = pushLayer(m, 'modal', noop);
    insertTour(t, m, noop);
    releaseM();
    expect(isTopmost(t)).toBe(false);
    // The page is free again: a new page tour is accepted.
    expect(insertTour(newLayerId('page'), null, noop)).not.toBeNull();
  });
});

describe('tourAbove', () => {
  it('is true only while the top layer is a tour lying above the layer', () => {
    const m = newLayerId('m');
    pushLayer(m, 'modal', noop);
    insertTour(newLayerId('t'), m, noop);
    expect(tourAbove(m)).toBe(true);
    const releaseP = pushLayer(newLayerId('p'), 'popover', noop);
    expect(tourAbove(m)).toBe(false);
    releaseP();
    expect(tourAbove(m)).toBe(true);
  });

  it('is false for a tour below the layer, and for a layer not in the stack', () => {
    const m = newLayerId('m');
    pushLayer(m, 'modal', noop);
    insertTour(newLayerId('t'), null, noop);
    expect(tourAbove(m)).toBe(false);
    expect(tourAbove(newLayerId('gone'))).toBe(false);
  });
});

describe('Escape router', () => {
  it('calls the top layer only', () => {
    const lower = vi.fn();
    const upper = vi.fn();
    pushLayer(newLayerId('lower'), 'modal', lower);
    pushLayer(newLayerId('upper'), 'popover', upper);
    const e = pressEscape();
    expect(upper).toHaveBeenCalledTimes(1);
    expect(lower).not.toHaveBeenCalled();
    expect(e.defaultPrevented).toBe(true);
  });

  it('leaves an Escape an element already consumed', () => {
    const top = vi.fn();
    pushLayer(newLayerId('top'), 'modal', top);
    const consume = (e: KeyboardEvent) => e.preventDefault();
    window.addEventListener('keydown', consume, true);
    try {
      pressEscape();
    } finally {
      window.removeEventListener('keydown', consume, true);
    }
    expect(top).not.toHaveBeenCalled();
  });

  it('a top layer that must not close still swallows the key', () => {
    const lower = vi.fn();
    pushLayer(newLayerId('lower'), 'modal', lower);
    pushLayer(newLayerId('busy'), 'modal', noop);
    expect(pressEscape().defaultPrevented).toBe(true);
    expect(lower).not.toHaveBeenCalled();
  });

  it('ignores other keys and stops listening once the stack is empty', () => {
    const top = vi.fn();
    const release = pushLayer(newLayerId('top'), 'modal', top);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(top).not.toHaveBeenCalled();
    release();
    expect(pressEscape().defaultPrevented).toBe(false);
  });
});

describe('onLayersChange', () => {
  it('notifies on every change and stops after unsubscribe', () => {
    const fn = vi.fn();
    const off = onLayersChange(fn);
    const release = pushLayer(newLayerId('a'), 'popover', noop);
    release();
    expect(fn).toHaveBeenCalledTimes(2);
    off();
    pushLayer(newLayerId('b'), 'popover', noop);
    expect(fn).toHaveBeenCalledTimes(2);
  });
});
