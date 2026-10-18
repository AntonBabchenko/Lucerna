import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FLASH_MS, fieldFlash } from '$lib/ui/field-flash';

function mountHost() {
  const host = document.createElement('div');
  const input = document.createElement('input');
  host.appendChild(input);
  document.body.appendChild(host);
  return { host, input };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = '';
});

describe('fieldFlash', () => {
  it('marks the node when it mounts already active', () => {
    const { host } = mountHost();
    fieldFlash(host, { active: true });
    expect(host.classList.contains('field-flash')).toBe(true);
  });

  it('does nothing while inactive', () => {
    const { host } = mountHost();
    fieldFlash(host, { active: false });
    expect(host.classList.contains('field-flash')).toBe(false);
  });

  it('clears the mark once the flash window elapses', () => {
    const { host } = mountHost();
    fieldFlash(host, { active: true });
    vi.advanceTimersByTime(FLASH_MS + 1);
    expect(host.classList.contains('field-flash')).toBe(false);
  });

  it('times the highlight animation by the same window that clears the mark', () => {
    const { host } = mountHost();
    fieldFlash(host, { active: true });
    expect(host.style.getPropertyValue('--field-flash-duration')).toBe(`${FLASH_MS}ms`);
  });

  it('moves focus into the node when focus is requested', () => {
    const { host, input } = mountHost();
    fieldFlash(host, { active: true, focus: true });
    expect(document.activeElement).toBe(input);
  });

  it('leaves focus alone by default — a stray arrow key must not edit a slider', () => {
    const { host, input } = mountHost();
    fieldFlash(host, { active: true });
    expect(document.activeElement).not.toBe(input);
  });

  it('re-flashes when active flips false then true again', () => {
    const { host } = mountHost();
    const handle = fieldFlash(host, { active: true });
    vi.advanceTimersByTime(FLASH_MS + 1);
    handle.update({ active: false });
    handle.update({ active: true });
    expect(host.classList.contains('field-flash')).toBe(true);
  });

  it('does not re-flash when update repeats the same active value', () => {
    const { host } = mountHost();
    const handle = fieldFlash(host, { active: true });
    host.classList.remove('field-flash');
    handle.update({ active: true });
    expect(host.classList.contains('field-flash')).toBe(false);
  });

  it('drops the mark and the pending timer on destroy', () => {
    const { host } = mountHost();
    const handle = fieldFlash(host, { active: true });
    handle.destroy();
    expect(host.classList.contains('field-flash')).toBe(false);
  });

  it('reports delivery in a microtask, once per false→true edge', async () => {
    const onDelivered = vi.fn();
    const { host } = mountHost();
    const a = fieldFlash(host, { active: true, onDelivered });
    // Not inside the call: the action's own mount call must return first.
    expect(onDelivered).not.toHaveBeenCalled();
    await Promise.resolve();
    expect(onDelivered).toHaveBeenCalledTimes(1);
    a.update({ active: false, onDelivered });
    a.update({ active: true, onDelivered });
    await Promise.resolve();
    expect(onDelivered).toHaveBeenCalledTimes(2);
  });

  it('a wrapper that mounts active, is consumed, and is asked again still flashes', async () => {
    const { host } = mountHost();
    let active = true;
    const a = fieldFlash(host, {
      active,
      onDelivered: () => {
        active = false;
      },
    });
    await Promise.resolve();
    // The consumed value the render effect now observes.
    a.update({ active });
    host.classList.remove('field-flash');
    a.update({ active: true });
    expect(host.classList.contains('field-flash')).toBe(true);
  });

  it('deactivating does not cut a running highlight short', () => {
    const { host } = mountHost();
    const a = fieldFlash(host, { active: true });
    a.update({ active: false });
    expect(host.classList.contains('field-flash')).toBe(true);
    vi.advanceTimersByTime(FLASH_MS);
    expect(host.classList.contains('field-flash')).toBe(false);
  });

  it('focus prefers the marked control, skips a disabled first control, and opens its disclosure first', () => {
    const host = document.createElement('div');
    host.innerHTML =
      '<button disabled>first</button><details><summary>more</summary><input data-flash-focus /></details>';
    document.body.appendChild(host);
    const calls: string[] = [];
    const details = host.querySelector('details') as HTMLDetailsElement;
    Object.defineProperty(details, 'open', {
      get: () => calls.includes('open'),
      set: () => {
        calls.push('open');
      },
      configurable: true,
    });
    host.scrollIntoView = () => {
      calls.push('scroll');
    };
    fieldFlash(host, { active: true, focus: true });
    expect(document.activeElement).toBe(host.querySelector('input'));
    expect(calls).toEqual(['open', 'scroll']);
  });

  it('a disabled target is focused the moment it enables', async () => {
    const host = document.createElement('div');
    host.innerHTML = '<input data-flash-focus disabled />';
    document.body.appendChild(host);
    const input = host.querySelector('input') as HTMLInputElement;
    fieldFlash(host, { active: true, focus: true });
    expect(document.activeElement).not.toBe(input);
    input.disabled = false;
    // MutationObserver delivers in a microtask.
    await Promise.resolve();
    await Promise.resolve();
    expect(document.activeElement).toBe(input);
  });

  it('a block taller than its scrollport aligns to its start; a short one centres', () => {
    const parent = document.createElement('div');
    parent.setAttribute('style', 'overflow-y: auto');
    Object.defineProperty(parent, 'clientHeight', { value: 400, configurable: true });
    const host = document.createElement('div');
    parent.appendChild(host);
    document.body.appendChild(parent);
    const scroll = vi.fn();
    host.scrollIntoView = scroll;
    host.getBoundingClientRect = () => ({ height: 5000 }) as DOMRect;
    fieldFlash(host, { active: true });
    expect(scroll).toHaveBeenLastCalledWith({ block: 'start' });
    host.getBoundingClientRect = () => ({ height: 50 }) as DOMRect;
    const b = fieldFlash(host, { active: false });
    b.update({ active: true });
    expect(scroll).toHaveBeenLastCalledWith({ block: 'center' });
  });

  // The Installed list's «show in the list» scrolls as its own jump always did — smoothly — and
  // Settings keep the browser's default.
  it('scrolls with the behaviour it is given, and the default when none', () => {
    const parent = document.createElement('div');
    parent.setAttribute('style', 'overflow-y: auto');
    Object.defineProperty(parent, 'clientHeight', { value: 400, configurable: true });
    const host = document.createElement('div');
    parent.appendChild(host);
    document.body.appendChild(parent);
    const scroll = vi.fn();
    host.scrollIntoView = scroll;
    host.getBoundingClientRect = () => ({ height: 50 }) as DOMRect;
    const a = fieldFlash(host, { active: true, behavior: 'smooth' });
    expect(scroll).toHaveBeenLastCalledWith({ block: 'center', behavior: 'smooth' });
    a.update({ active: false });
    a.update({ active: true });
    expect(scroll.mock.lastCall?.[0]).toEqual({ block: 'center' });
  });

  it('parks focus on the wrapper while the target is disabled, then hands it over', async () => {
    const host = document.createElement('div');
    host.innerHTML = '<input data-flash-focus disabled />';
    document.body.appendChild(host);
    const input = host.querySelector('input') as HTMLInputElement;
    fieldFlash(host, { active: true, focus: true });
    // Parked inside the wrapper: a dialog's deferred initial focus sees focus
    // already inside and leaves it alone.
    expect(document.activeElement).toBe(host);
    input.disabled = false;
    await Promise.resolve();
    await Promise.resolve();
    expect(document.activeElement).toBe(input);
  });

  it('with only disabled controls, parks on the wrapper and hands focus to the first once it enables', async () => {
    // A jump whose link unmounted must not leave focus on <body>: the settings
    // behind the control may still be loading.
    const host = document.createElement('div');
    host.innerHTML = '<input type="checkbox" disabled /><button disabled>b</button>';
    document.body.appendChild(host);
    const box = host.querySelector('input') as HTMLInputElement;
    fieldFlash(host, { active: true, focus: true });
    expect(document.activeElement).toBe(host);
    box.disabled = false;
    await Promise.resolve();
    await Promise.resolve();
    expect(document.activeElement).toBe(box);
  });

  it('does not steal focus on enable when something else claimed it meanwhile', async () => {
    const host = document.createElement('div');
    host.innerHTML = '<input data-flash-focus disabled />';
    document.body.appendChild(host);
    const other = document.createElement('button');
    document.body.appendChild(other);
    const input = host.querySelector('input') as HTMLInputElement;
    fieldFlash(host, { active: true, focus: true });
    other.focus();
    input.disabled = false;
    await Promise.resolve();
    await Promise.resolve();
    expect(document.activeElement).toBe(other);
  });
});

// A jump into a section whose content is still loading (Settings → Game's Java rows read the
// detected Javas first) used to scroll once, at delivery: the content above grew, the rows loaded,
// and the field ended below the fold while its flash played off-screen (v0.28 regression, F3).
describe('fieldFlash — keeps the field in view while the layout settles', () => {
  class FakeResizeObserver {
    static all: FakeResizeObserver[] = [];
    observed: Element[] = [];
    disconnected = false;
    constructor(private cb: () => void) {
      FakeResizeObserver.all.push(this);
    }
    observe(el: Element) {
      this.observed.push(el);
    }
    disconnect() {
      this.disconnected = true;
    }
    fire() {
      if (!this.disconnected) this.cb();
    }
  }
  const fireAll = () => {
    for (const o of FakeResizeObserver.all) o.fire();
  };

  function mountScrolled() {
    const parent = document.createElement('div');
    parent.setAttribute('style', 'overflow-y: auto');
    Object.defineProperty(parent, 'clientHeight', { value: 400, configurable: true });
    const above = document.createElement('div');
    const host = document.createElement('div');
    parent.append(above, host);
    document.body.appendChild(parent);
    const scroll = vi.fn();
    host.scrollIntoView = scroll;
    host.getBoundingClientRect = () => ({ height: 50 }) as DOMRect;
    return { parent, above, host, scroll };
  }

  beforeEach(() => {
    FakeResizeObserver.all = [];
    vi.stubGlobal('ResizeObserver', FakeResizeObserver);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('re-aligns the field when a size changes during the flash', () => {
    const { above, host, scroll } = mountScrolled();
    fieldFlash(host, { active: true });
    expect(scroll).toHaveBeenCalledTimes(1);
    const watched = FakeResizeObserver.all.flatMap((o) => o.observed);
    expect(watched).toContain(host);
    expect(watched).toContain(above);
    fireAll();
    expect(scroll).toHaveBeenCalledTimes(2);
    expect(scroll).toHaveBeenLastCalledWith({ block: 'center' });
  });

  it('stops once the user takes the scroll', () => {
    const { parent, host, scroll } = mountScrolled();
    fieldFlash(host, { active: true });
    expect(FakeResizeObserver.all.length).toBeGreaterThan(0);
    parent.dispatchEvent(new Event('wheel'));
    fireAll();
    expect(scroll).toHaveBeenCalledTimes(1);
  });

  it('also watches the field’s ancestors below the scroll container', () => {
    // A column beside a taller one: content growing above the field changes the column, not the
    // scroll container's child that holds both columns.
    const parent = document.createElement('div');
    parent.setAttribute('style', 'overflow-y: auto');
    Object.defineProperty(parent, 'clientHeight', { value: 400, configurable: true });
    const grid = document.createElement('div');
    const column = document.createElement('div');
    const host = document.createElement('div');
    column.appendChild(host);
    grid.appendChild(column);
    parent.appendChild(grid);
    document.body.appendChild(parent);
    host.scrollIntoView = vi.fn();
    host.getBoundingClientRect = () => ({ height: 50 }) as DOMRect;
    fieldFlash(host, { active: true });
    const watched = FakeResizeObserver.all.flatMap((o) => o.observed);
    expect(watched).toContain(column);
    expect(watched).toContain(grid);
    expect(watched).not.toContain(parent);
  });

  it.each([
    'pointerdown',
    'keydown',
    'touchstart',
  ])('a %s in the scroll container hands the scroll back', (type) => {
    const { parent, host, scroll } = mountScrolled();
    fieldFlash(host, { active: true });
    expect(FakeResizeObserver.all.length).toBeGreaterThan(0);
    parent.dispatchEvent(new Event(type));
    fireAll();
    expect(scroll).toHaveBeenCalledTimes(1);
  });

  it('a re-flash while running leaves exactly one live watcher', () => {
    const { host } = mountScrolled();
    const action = fieldFlash(host, { active: true });
    action.update({ active: false });
    action.update({ active: true });
    expect(FakeResizeObserver.all.filter((o) => !o.disconnected)).toHaveLength(1);
  });

  it('stops when the flash ends, and on destroy', () => {
    const a = mountScrolled();
    fieldFlash(a.host, { active: true });
    expect(FakeResizeObserver.all.length).toBeGreaterThan(0);
    vi.advanceTimersByTime(FLASH_MS + 1);
    expect(FakeResizeObserver.all.every((o) => o.disconnected)).toBe(true);
    fireAll();
    expect(a.scroll).toHaveBeenCalledTimes(1);

    FakeResizeObserver.all = [];
    const b = mountScrolled();
    const action = fieldFlash(b.host, { active: true });
    expect(FakeResizeObserver.all.length).toBeGreaterThan(0);
    action.destroy();
    expect(FakeResizeObserver.all.every((o) => o.disconnected)).toBe(true);
  });
});
