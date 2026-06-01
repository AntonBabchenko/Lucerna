import { describe, it, expect, beforeEach, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    updateInstall: vi.fn(async () => ({ status: 'ok', data: null })),
    updateDismiss: vi.fn(async () => ({ status: 'ok', data: null })),
  },
}));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn(async () => {}) }));

import { updateState, dismissUpdate } from '$lib/update/state.svelte';
import { commands } from '$lib/ipc/bindings';

describe('update state', () => {
  beforeEach(() => {
    updateState.value = null;
    vi.clearAllMocks();
  });

  it('dismissUpdate clears the rune and persists the version', async () => {
    updateState.value = { latest: '0.9.1' } as never;
    await dismissUpdate('0.9.1');
    expect(commands.updateDismiss).toHaveBeenCalledWith('0.9.1');
    expect(updateState.value).toBeNull();
  });
});
