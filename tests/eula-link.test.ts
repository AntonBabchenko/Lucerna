import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

import EulaLink from '$lib/servers/EulaLink.svelte';
import { EULA_URL } from '$lib/servers/eula-link';

describe('EulaLink', () => {
  beforeEach(() => {
    openUrlMock.mockClear(); // module-level mock accumulates across tests
  });

  it('opens the canonical Minecraft EULA URL via tauri-plugin-opener', async () => {
    render(EulaLink);

    await fireEvent.click(screen.getByTestId('eula-link'));

    // The opener is dynamic-imported inside an awaited promise — let it flush.
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith('https://aka.ms/MinecraftEULA');
    });
  });

  it('points at the same URL the Rust side writes into eula.txt', () => {
    // src-tauri/src/servers_runtime/eula.rs writes this link into the file the
    // user is agreeing to; the two must not drift apart.
    expect(EULA_URL).toBe('https://aka.ms/MinecraftEULA');
  });

  it('uses the caller-supplied label when one is given', () => {
    render(EulaLink, { props: { label: 'Read the Minecraft EULA' } });
    expect(screen.getByTestId('eula-link').textContent).toContain('Read the Minecraft EULA');
  });
});
