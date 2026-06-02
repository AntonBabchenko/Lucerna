import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ExportModInfo, ExportPreview } from '$lib/ipc/bindings';

// --- module mocks -----------------------------------------------------------
// exportPreview / exportModpack are reset per test via mockResolvedValue.
const { exportPreview, exportModpack, save, pushSuccess, pushWarning } = vi.hoisted(() => ({
  exportPreview: vi.fn(),
  exportModpack: vi.fn(),
  save: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({ commands: { exportPreview, exportModpack } }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save }));
// The component constructs `new Channel()` and assigns `.onmessage`; a bare
// class stub is enough — we don't drive progress messages here.
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({ pushSuccess, pushWarning }));

import ExportPackDialog from '$lib/modpacks/ExportPackDialog.svelte';

// --- fixtures ---------------------------------------------------------------
function mod(overrides: Partial<ExportModInfo> = {}): ExportModInfo {
  return {
    sha1: 'a'.repeat(40),
    name: 'Sodium',
    filename: 'sodium.jar',
    source: 'modrinth',
    has_ids: true,
    ...overrides,
  };
}

function preview(overrides: Partial<ExportPreview> = {}): ExportPreview {
  return {
    mods: [mod()],
    has_config: false,
    has_resourcepacks: false,
    has_shaderpacks: false,
    has_saves: false,
    saves_size_bytes: null,
    ...overrides,
  };
}

const ok = <T>(data: T) => ({ status: 'ok', data }) as const;
const err = <E>(error: E) => ({ status: 'error', error }) as const;

function renderDialog(
  props: Partial<{ instanceId: string; instanceName: string; onClose: () => void }> = {},
) {
  const onClose = vi.fn();
  render(ExportPackDialog, {
    props: { instanceId: 'inst-1', instanceName: 'My Pack', onClose, ...props },
  });
  return { onClose };
}

afterEach(() => {
  vi.clearAllMocks();
});

describe('ExportPackDialog', () => {
  it('shows the loading label until the preview resolves', () => {
    // A pending preview keeps the dialog in its loading state.
    exportPreview.mockReturnValue(new Promise(() => {}));
    renderDialog();
    expect(screen.getByText('Loading…')).toBeTruthy();
    expect(screen.queryByText('Format')).toBeNull();
  });

  it('renders the formatted error and no form when the preview fails', async () => {
    exportPreview.mockResolvedValue(err({ kind: 'instance_not_found', id: 'inst-1' }));
    renderDialog();
    expect(await screen.findByText('Instance inst-1 not found')).toBeTruthy();
    expect(screen.queryByText('Format')).toBeNull();
    expect(screen.queryByText('Loading…')).toBeNull();
  });

  it('renders only the content rows the preview advertises', async () => {
    exportPreview.mockResolvedValue(
      ok(
        preview({
          has_config: true,
          has_resourcepacks: true,
          has_shaderpacks: true,
          has_saves: true,
        }),
      ),
    );
    renderDialog();
    expect(await screen.findByText('Format')).toBeTruthy();
    expect(screen.getByText('Mods (1)')).toBeTruthy();
    expect(screen.getByText('Configs')).toBeTruthy();
    expect(screen.getByText('Resource packs')).toBeTruthy();
    expect(screen.getByText('Shader packs')).toBeTruthy();
    expect(screen.getByText('Worlds')).toBeTruthy();
  });

  it('hides content rows the preview does not advertise', async () => {
    exportPreview.mockResolvedValue(ok(preview()));
    renderDialog();
    expect(await screen.findByText('Mods (1)')).toBeTruthy();
    expect(screen.queryByText('Configs')).toBeNull();
    expect(screen.queryByText('Resource packs')).toBeNull();
    expect(screen.queryByText('Worlds')).toBeNull();
  });

  it('disables Export when there are zero mods', async () => {
    exportPreview.mockResolvedValue(ok(preview({ mods: [] })));
    renderDialog();
    expect(await screen.findByText('Format')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Export' })).toHaveProperty('disabled', true);
  });

  it('disables Export when the pack name is blank', async () => {
    exportPreview.mockResolvedValue(ok(preview()));
    renderDialog({ instanceName: '' });
    expect(await screen.findByText('Format')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Export' })).toHaveProperty('disabled', true);
  });

  it('enables Export for a valid pack', async () => {
    exportPreview.mockResolvedValue(ok(preview()));
    renderDialog();
    expect(await screen.findByText('Format')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Export' })).toHaveProperty('disabled', false);
  });

  it('calls onClose when Cancel is clicked', async () => {
    exportPreview.mockResolvedValue(ok(preview()));
    const { onClose } = renderDialog();
    await fireEvent.click(await screen.findByText('Cancel'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('shows the worlds warning only once worlds are checked', async () => {
    exportPreview.mockResolvedValue(ok(preview({ has_saves: true })));
    renderDialog();
    const worlds = (await screen.findByText('Worlds')).closest('label')?.querySelector('input');
    expect(screen.queryByText(/Worlds may contain personal builds/)).toBeNull();
    await fireEvent.click(worlds as HTMLInputElement);
    expect(screen.getByText(/Worlds may contain personal builds/)).toBeTruthy();
  });

  it('lists mods that cannot be referenced in lightweight mode', async () => {
    // A local jar with no platform IDs is unresolvable in lightweight mode.
    exportPreview.mockResolvedValue(
      ok(preview({ mods: [mod({ name: 'LocalOnly', has_ids: false })] })),
    );
    renderDialog();
    expect(await screen.findByText('Mods without a download link')).toBeTruthy();
    expect(screen.getByText('LocalOnly')).toBeTruthy();
  });

  it('runs the export and reports success', async () => {
    exportPreview.mockResolvedValue(ok(preview()));
    exportModpack.mockResolvedValue(ok(null));
    save.mockResolvedValue('/out/my-pack-1.0.0.mrpack');
    const { onClose } = renderDialog();

    await fireEvent.click(await screen.findByRole('button', { name: 'Export' }));

    await waitFor(() => expect(exportModpack).toHaveBeenCalledTimes(1));
    const [instanceId, options, dest] = exportModpack.mock.calls[0];
    expect(instanceId).toBe('inst-1');
    expect(dest).toBe('/out/my-pack-1.0.0.mrpack');
    expect(options).toMatchObject({
      format: 'modrinth',
      mode: 'lightweight',
      include_config: true,
      bundle_shas: [],
      metadata: { name: 'My Pack', version: '1.0.0', author: '', summary: '' },
    });
    await waitFor(() => expect(pushSuccess).toHaveBeenCalled());
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('does not export when the save dialog is cancelled', async () => {
    exportPreview.mockResolvedValue(ok(preview()));
    save.mockResolvedValue(null);
    const { onClose } = renderDialog();

    await fireEvent.click(await screen.findByRole('button', { name: 'Export' }));

    await waitFor(() => expect(save).toHaveBeenCalled());
    expect(exportModpack).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('warns and stays open when the export fails', async () => {
    exportPreview.mockResolvedValue(ok(preview()));
    exportModpack.mockResolvedValue(err({ kind: 'io', path: '/out', details: 'disk full' }));
    save.mockResolvedValue('/out/my-pack-1.0.0.mrpack');
    const { onClose } = renderDialog();

    await fireEvent.click(await screen.findByRole('button', { name: 'Export' }));

    await waitFor(() => expect(pushWarning).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
    // The dialog must re-enable Export after a failure so the user can retry —
    // i.e. `busy` is cleared on the error branch, not only on success.
    expect(screen.getByRole('button', { name: 'Export' })).toHaveProperty('disabled', false);
  });

  it('disables Export when the version is blank', async () => {
    exportPreview.mockResolvedValue(ok(preview()));
    renderDialog();
    expect(await screen.findByText('Format')).toBeTruthy();
    await fireEvent.input(screen.getByPlaceholderText('Version'), { target: { value: '' } });
    expect(screen.getByRole('button', { name: 'Export' })).toHaveProperty('disabled', true);
  });

  it('hides the unresolvable list and sends bundle_shas=[] in full mode', async () => {
    // A local jar (no IDs) is unresolvable in lightweight mode and listed; full
    // mode bundles everything via overrides, so the checklist is gone and the
    // command receives an empty bundle_shas regardless of unresolvable mods.
    exportPreview.mockResolvedValue(
      ok(preview({ mods: [mod({ name: 'LocalOnly', has_ids: false })] })),
    );
    exportModpack.mockResolvedValue(ok(null));
    save.mockResolvedValue('/out/my-pack-1.0.0.mrpack');
    renderDialog();

    expect(await screen.findByText('Mods without a download link')).toBeTruthy();
    await fireEvent.click(screen.getByLabelText('Full (bundle everything)'));
    expect(screen.queryByText('Mods without a download link')).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: 'Export' }));
    await waitFor(() => expect(exportModpack).toHaveBeenCalledTimes(1));
    const options = exportModpack.mock.calls[0][1];
    expect(options.mode).toBe('full');
    expect(options.bundle_shas).toEqual([]);
  });
});
