import { get } from 'svelte/store';
import { describe, expect, it } from 'vitest';
import { t } from '$lib/i18n';
import type { Fallback } from '$lib/ipc/bindings';
import { fallbackMessage, fallbackOf } from '$lib/settings/fallback-message';

// Real translator (not a stub), so these pin the ACTUAL copy.
const tr = get(t);

const TAIL = 'nothing here is saved to your data folder';

describe('fallbackMessage', () => {
  it('names the folder and tells a missing one apart from one that cannot be written to', () => {
    const missing = fallbackMessage(tr, { kind: 'root_missing' }, 'D:\\LucernaData');
    expect(missing).toContain('"D:\\LucernaData" can\'t be found');
    expect(missing).toContain('reconnect it');

    const readOnly = fallbackMessage(
      tr,
      { kind: 'root_not_writable', details: 'Access is denied.' },
      'D:\\LucernaData',
    );
    expect(readOnly).toContain("can't be written to");
    // A folder that is plugged in must never be told to "reconnect".
    expect(readOnly).not.toMatch(/reconnect/i);
    // Details belong to the Storage notice, not to the banner sentence.
    expect(readOnly).not.toContain('Access is denied');
  });

  it('says it could not check, rather than guessing, when the probe failed', () => {
    const unknown = fallbackMessage(tr, { kind: 'root_unknown', details: 'io' }, 'D:\\X');
    expect(unknown).toContain("couldn't check");
    expect(unknown).not.toMatch(/reconnect/i);
  });

  it.each<Fallback>([
    { kind: 'pointer_corrupt' },
    { kind: 'pointer_unreadable', details: 'Access is denied.' },
  ])('has a sentence without a path for $kind — there is no path to name', (reason) => {
    const text = fallbackMessage(tr, reason, null);
    expect(text).toContain("couldn't read where your data folder is");
    expect(text).not.toContain('""');
    expect(text).not.toContain('{path}');
  });

  it('always ends by saying the session is temporary — and never calls it empty', () => {
    for (const reason of [
      { kind: 'root_missing' },
      { kind: 'pointer_corrupt' },
    ] satisfies Fallback[]) {
      const text = fallbackMessage(tr, reason, 'D:\\X');
      expect(text).toContain(TAIL);
      expect(text).not.toMatch(/empty/i);
    }
  });
});

describe('fallbackOf', () => {
  const base = { effective: 'x', configured: 'D:\\X', default_dir: 'C:\\Default' };

  it('is null when the launcher runs from its data folder', () => {
    expect(fallbackOf(null)).toBeNull();
    expect(fallbackOf({ ...base, fell_back: false, fallback: null })).toBeNull();
  });

  it('passes the reason through', () => {
    const reason: Fallback = { kind: 'root_not_writable', details: 'ro' };
    expect(fallbackOf({ ...base, fell_back: true, fallback: reason })).toEqual(reason);
  });

  it('reads an old-shaped status (fell_back without a reason) as a missing folder', () => {
    // Six unit suites and older mocks still feed this shape; it must neither crash the new UI
    // nor hide the fallback.
    expect(fallbackOf({ ...base, fell_back: true })).toEqual({ kind: 'root_missing' });
  });
});
