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
