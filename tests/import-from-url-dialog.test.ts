import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modpackResolveUrl: vi.fn(),
  },
}));

vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: (k: string, p?: Record<string, unknown>) => string) => void) => {
      run((k: string) => k);
      return () => {};
    },
  },
}));

vi.mock('$lib/ipc/format-error', () => ({
  formatError: (e: { kind: string }) => `formatted:${e.kind}`,
}));

import type { ModpackHit } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import ImportFromUrlDialog from '$lib/modpacks/ImportFromUrlDialog.svelte';

function hit(over: Partial<ModpackHit> = {}): ModpackHit {
  return {
    project_id: 'PaCk1',
    slug: 'cobblemon',
    title: 'Cobblemon',
    description: 'Pokemon-ish',
    icon_url: null,
    downloads: 100,
    latest_mc_version: '1.21.1',
    supported_loaders: ['fabric'],
    source: 'modrinth',
    distribution_allowed: null,
    author: null,
    ...over,
  } as ModpackHit;
}

describe('ImportFromUrlDialog', () => {
  beforeEach(() => {
    vi.mocked(commands.modpackResolveUrl).mockReset();
  });

  it('keeps Continue disabled until a link is entered', async () => {
    const { getByTestId } = render(ImportFromUrlDialog, {
      props: { onCancel: vi.fn(), onResolved: vi.fn() },
    });
    const submit = getByTestId('import-url-submit') as HTMLButtonElement;
    expect(submit.disabled).toBe(true);

    await fireEvent.input(getByTestId('import-url-input'), {
      target: { value: 'https://modrinth.com/modpack/cobblemon' },
    });
    expect((getByTestId('import-url-submit') as HTMLButtonElement).disabled).toBe(false);
  });

  it('hands the resolved pack and version up on success', async () => {
    const onResolved = vi.fn();
    vi.mocked(commands.modpackResolveUrl).mockResolvedValue({
      status: 'ok',
      data: { hit: hit(), version_id: 'v9' },
    } as never);

    const { getByTestId } = render(ImportFromUrlDialog, {
      props: { prefill: 'https://modrinth.com/modpack/cobblemon', onCancel: vi.fn(), onResolved },
    });
    await fireEvent.click(getByTestId('import-url-submit'));

    await waitFor(() =>
      expect(onResolved).toHaveBeenCalledWith(expect.objectContaining({ slug: 'cobblemon' }), 'v9'),
    );
  });

  it('trims the pasted link before resolving', async () => {
    vi.mocked(commands.modpackResolveUrl).mockResolvedValue({
      status: 'ok',
      data: { hit: hit(), version_id: null },
    } as never);

    const { getByTestId } = render(ImportFromUrlDialog, {
      props: {
        prefill: '  https://modrinth.com/modpack/cobblemon  ',
        onCancel: vi.fn(),
        onResolved: vi.fn(),
      },
    });
    await fireEvent.click(getByTestId('import-url-submit'));

    await waitFor(() =>
      expect(commands.modpackResolveUrl).toHaveBeenCalledWith(
        'https://modrinth.com/modpack/cobblemon',
      ),
    );
  });

  it('shows the formatted error and does not resolve on failure', async () => {
    const onResolved = vi.fn();
    vi.mocked(commands.modpackResolveUrl).mockResolvedValue({
      status: 'error',
      error: { kind: 'import_url_invalid', reason: 'nope' },
    } as never);

    const { getByTestId } = render(ImportFromUrlDialog, {
      props: { prefill: 'javascript:alert(1)', onCancel: vi.fn(), onResolved },
    });
    await fireEvent.click(getByTestId('import-url-submit'));

    await waitFor(() =>
      expect(getByTestId('import-url-error').textContent).toContain('formatted:import_url_invalid'),
    );
    expect(onResolved).not.toHaveBeenCalled();
  });

  it('says so when the link came from another app', () => {
    const { getByTestId } = render(ImportFromUrlDialog, {
      props: {
        prefill: 'lucerna://modpack?source=modrinth&project=x',
        fromExternal: true,
        onCancel: vi.fn(),
        onResolved: vi.fn(),
      },
    });
    expect(getByTestId('import-url-external-note')).toBeTruthy();
  });

  it('omits the external note for a link the user pasted', () => {
    const { queryByTestId } = render(ImportFromUrlDialog, {
      props: { onCancel: vi.fn(), onResolved: vi.fn() },
    });
    expect(queryByTestId('import-url-external-note')).toBeNull();
  });
});
