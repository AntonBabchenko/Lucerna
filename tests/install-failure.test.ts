import { get } from 'svelte/store';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale, t } from '$lib/i18n';

const toasts = vi.hoisted(() => ({ action: vi.fn().mockReturnValue(7) }));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushActionToast: (...a: unknown[]) => toasts.action(...a) as number,
}));

import type { Error as IpcError } from '$lib/ipc/bindings';
import { installFailureToast } from '$lib/mods/install-failure';

describe('installFailureToast', () => {
  beforeAll(() => locale.set('en'));

  it('pushes a sticky warning with mod name, localized cause and a Retry action', () => {
    const retry = vi.fn();
    const err = {
      kind: 'mods_network',
      url: 'https://cdn.example/x.jar',
      details: 'timed out',
    } as IpcError;
    const id = installFailureToast('Sodium', err, retry);
    expect(id).toBe(7);
    expect(toasts.action).toHaveBeenCalledTimes(1);
    const [kind, title, action, lines] = toasts.action.mock.calls[0] ?? [];
    expect(kind).toBe('warning');
    expect(title).toBe(get(t)('mods.browse.toastInstallFailedWithMod', { name: 'Sodium' }));
    expect(title).toContain('Sodium');
    expect((action as { label: string }).label).toBe(get(t)('mods.browse.toastRetry'));
    // Cause line comes from formatError — non-empty, localized.
    expect(Array.isArray(lines)).toBe(true);
    expect((lines as string[]).length).toBe(1);
    expect((lines as string[])[0]?.length).toBeGreaterThan(0);
    expect(retry).not.toHaveBeenCalled();
    (action as { run: () => void }).run();
    expect(retry).toHaveBeenCalledTimes(1);
  });
});
