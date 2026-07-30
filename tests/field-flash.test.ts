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

  it('drops the mark and the pending timer on destroy', () => {
    const { host } = mountHost();
    const handle = fieldFlash(host, { active: true });
    handle.destroy();
    expect(host.classList.contains('field-flash')).toBe(false);
  });
});
