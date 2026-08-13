// tests/updates-panel.test.ts
import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        general: {
          hide_to_tray_during_game: false,
          theme: 'system',
          check_updates_on_startup: true,
          gpu_preference: 'auto',
        },
      },
    }),
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    updateCheck: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { available: false, current: '0.0.0' } }),
  },
}));

import UpdatesPanel from '../src/lib/settings/UpdatesPanel.svelte';

describe('UpdatesPanel', () => {
  it('renders the check-on-startup toggle and the manual check button', () => {
    const { container } = render(UpdatesPanel);
    expect(container.querySelector('[data-testid="updates-toggle"]')).not.toBeNull();
    expect(screen.getByTestId('check-updates-btn')).toBeTruthy();
  });

  it("renders the changelog (What's new) with the always-present 0.1.0 entry", () => {
    render(UpdatesPanel);
    // Query the heading, not the bare text: this panel renders the real
    // CHANGELOG.md, and a release note that happens to name the "What's new"
    // panel puts a second element with that text on screen.
    expect(screen.getByRole('heading', { name: "What's new" })).toBeTruthy();
    expect(screen.getByText('v0.1.0')).toBeTruthy();
  });
});
