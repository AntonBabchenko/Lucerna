import { beforeEach, describe, expect, it, vi } from 'vitest';
import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';

const KEY = 'lucerna.diagnosisDismissed';

beforeEach(() => {
  localStorage.clear();
  diagnosisDismiss.reset();
});

describe('diagnosisDismiss', () => {
  it('defaults to not dismissed', () => {
    expect(diagnosisDismiss.isDismissed('server:s1', 'sig-a')).toBe(false);
  });

  it('dismisses exactly the signature it was given', () => {
    diagnosisDismiss.dismiss('server:s1', 'sig-a');
    expect(diagnosisDismiss.isDismissed('server:s1', 'sig-a')).toBe(true);
  });

  it('resurfaces when the signature changes (new/different problem)', () => {
    diagnosisDismiss.dismiss('server:s1', 'sig-a');
    // Same surface, different diagnosis identity → not suppressed.
    expect(diagnosisDismiss.isDismissed('server:s1', 'sig-b')).toBe(false);
  });

  it('keeps dismissal independent per key', () => {
    diagnosisDismiss.dismiss('server:s1', 'sig-a');
    expect(diagnosisDismiss.isDismissed('server:s1', 'sig-a')).toBe(true);
    expect(diagnosisDismiss.isDismissed('log:i1', 'sig-a')).toBe(false);
  });

  it('restore clears the dismissal', () => {
    diagnosisDismiss.dismiss('server:s1', 'sig-a');
    diagnosisDismiss.restore('server:s1');
    expect(diagnosisDismiss.isDismissed('server:s1', 'sig-a')).toBe(false);
  });

  it('never suppresses when the signature is null (unidentifiable)', () => {
    expect(diagnosisDismiss.isDismissed('server:s1', null)).toBe(false);
    diagnosisDismiss.dismiss('server:s1', null); // no-op
    expect(localStorage.getItem(KEY)).toBeNull();
  });

  it('persists the dismissed map to localStorage', () => {
    diagnosisDismiss.dismiss('server:s1', 'sig-a');
    const raw = localStorage.getItem(KEY);
    expect(JSON.parse(raw as string)).toEqual({ 'server:s1': 'sig-a' });
  });

  it('drops a key from localStorage when restored', () => {
    diagnosisDismiss.dismiss('server:s1', 'sig-a');
    diagnosisDismiss.restore('server:s1');
    expect(JSON.parse(localStorage.getItem(KEY) as string)).toEqual({});
  });

  it('reloads dismissed state from localStorage on a fresh import', async () => {
    localStorage.setItem(KEY, JSON.stringify({ 'log:i7': 'sig-z' }));
    vi.resetModules();
    const fresh = await import('$lib/ui/diagnosis-dismiss.svelte');
    expect(fresh.diagnosisDismiss.isDismissed('log:i7', 'sig-z')).toBe(true);
    expect(fresh.diagnosisDismiss.isDismissed('log:i7', 'sig-other')).toBe(false);
  });

  it('ignores a corrupt localStorage payload (array instead of object)', async () => {
    localStorage.setItem(KEY, JSON.stringify(['not', 'an', 'object']));
    vi.resetModules();
    const fresh = await import('$lib/ui/diagnosis-dismiss.svelte');
    expect(fresh.diagnosisDismiss.isDismissed('server:s1', 'sig-a')).toBe(false);
  });
});
