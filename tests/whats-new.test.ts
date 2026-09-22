import { beforeEach, describe, expect, it } from 'vitest';
import type { Changelog } from '$lib/changelog/types';
import { checkWhatsNew, whatsNewState } from '$lib/changelog/whats-new.svelte';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';

function v(version: string): Changelog[number] {
  return {
    version,
    date: '2026-01-01',
    url: null,
    sections: [{ kind: 'added', heading: 'Added', items: [`item ${version}`] }],
  };
}
const LOG: Changelog = [v('0.23.0'), v('0.22.0')];

describe('checkWhatsNew', () => {
  beforeEach(() => {
    for (const t of [...toastList()]) dismiss(t.id);
    whatsNewState.entries = null;
  });

  it('shows a prompt and marks seen when the version advanced', async () => {
    const marked: string[] = [];
    await checkWhatsNew('0.22.0', {
      entries: LOG,
      recoverySession: async () => false,
      currentVersion: async () => '0.23.0',
      markSeen: async (x) => void marked.push(x),
    });
    const toast = toastList().find((t) => t.action);
    expect(toast).toBeTruthy();
    expect(marked).toEqual(['0.23.0']);
    // Clicking the action opens the modal scoped to the new versions.
    toast!.action!.run();
    expect(whatsNewState.entries?.map((x) => x.version)).toEqual(['0.23.0']);
  });

  it('shows nothing on first-ever launch (seen == null) but records the baseline', async () => {
    const marked: string[] = [];
    await checkWhatsNew(null, {
      entries: LOG,
      recoverySession: async () => false,
      currentVersion: async () => '0.23.0',
      markSeen: async (x) => void marked.push(x),
    });
    expect(toastList().some((t) => t.action)).toBe(false);
    expect(marked).toEqual(['0.23.0']);
  });

  it('shows nothing and writes nothing when already on the seen version', async () => {
    const marked: string[] = [];
    await checkWhatsNew('0.23.0', {
      entries: LOG,
      recoverySession: async () => false,
      currentVersion: async () => '0.23.0',
      markSeen: async (x) => void marked.push(x),
    });
    expect(toastList().some((t) => t.action)).toBe(false);
    expect(marked).toEqual([]);
  });

  it('does nothing in a recovery session — not even the baseline write', async () => {
    // The throwaway root has no app.json of the user's: a baseline written there is lost, and a
    // prompt over the recovery banner is noise.
    const marked: string[] = [];
    await checkWhatsNew('0.22.0', {
      entries: LOG,
      recoverySession: async () => true,
      currentVersion: async () => '0.23.0',
      markSeen: async (x) => void marked.push(x),
    });
    expect(toastList().some((t) => t.action)).toBe(false);
    expect(marked).toEqual([]);
  });

  it('does nothing while it cannot tell which session this is', async () => {
    // "Could not tell" is restrictive: it is offered again on the next start.
    const marked: string[] = [];
    await checkWhatsNew('0.22.0', {
      entries: LOG,
      recoverySession: async () => {
        throw new Error('status unavailable');
      },
      currentVersion: async () => '0.23.0',
      markSeen: async (x) => void marked.push(x),
    });
    expect(toastList().some((t) => t.action)).toBe(false);
    expect(marked).toEqual([]);
  });
});
