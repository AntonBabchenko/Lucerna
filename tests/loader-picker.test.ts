import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import LoaderPicker from '$lib/instances/LoaderPicker.svelte';

// Mock the IPC commands module so the component doesn't try to call
// real Tauri commands during unit tests.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listFabricLoaders: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        { version: '0.16.0', stable: true },
        { version: '0.17.0-beta.1', stable: false },
      ],
    }),
    listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
}));

describe('LoaderPicker', () => {
  it('renders five loader-kind buttons', () => {
    const { getByText } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'vanilla', loaderVersion: null },
    });
    // Buttons show brand-canonical display names, not snake_case enum
    // values — see src/lib/instances/loader-display.ts.
    for (const k of ['Vanilla', 'Fabric', 'Quilt', 'Forge', 'NeoForge']) {
      expect(getByText(k)).toBeTruthy();
    }
  });

  it('does not show loader-version dropdown when loader is vanilla', () => {
    const { queryByLabelText } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'vanilla', loaderVersion: null },
    });
    expect(queryByLabelText(/loader version/i)).toBeFalsy();
  });

  it('shows loader-version dropdown when a non-vanilla loader is selected and versions are loaded', async () => {
    const { getByText, findByLabelText } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'vanilla', loaderVersion: null },
    });
    await fireEvent.click(getByText('Fabric'));
    // The $effect re-fetches; the await findByLabelText polls until the
    // dropdown materialises.
    const select = await findByLabelText(/loader version/i);
    expect(select).toBeTruthy();
  });
});
