// The tray's words live in a pure module so they can be tested; +page.svelte,
// which sends them, cannot be mounted in isolation.
import { get } from 'svelte/store';
import { describe, expect, it } from 'vitest';
import { t } from '$lib/i18n';
import { trayLabels, trayRefusalKey } from '$lib/tray/tray-copy';

describe('the tray menu words', () => {
  it('are the interface strings, not the English fallback the backend builds with', () => {
    const labels = trayLabels(get(t));
    expect(labels).toEqual({
      open: 'Open Lucerna',
      quit: 'Quit Lucerna',
      tooltip_running: 'Lucerna — a game is running',
    });
  });

  it('name each refusal by its own reason', () => {
    // A swapped pair would tell a user whose game is running to "wait for the
    // operation", and one whose check failed that "Minecraft is running".
    expect(trayRefusalKey('running')).toBe('tray.blocked.running');
    expect(trayRefusalKey('busy')).toBe('tray.blocked.busy');
    expect(trayRefusalKey('unknown')).toBe('tray.blocked.unknown');
  });
});
