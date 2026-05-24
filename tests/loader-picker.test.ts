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

  it('preserves a non-stable loaderVersion passed by the parent on mount', async () => {
    // Regression: load() used to unconditionally overwrite loaderVersion
    // with the stable entry after the fetch resolved, even when the
    // parent had just passed a valid non-stable choice (the user's
    // previously committed pick). On every modal reopen the dropdown
    // visually reverted to "(recommended)" — the silent UI lie users
    // saw as "I can't pick anything but recommended."
    //
    // The mock returns [0.16.0 (stable), 0.17.0-beta.1 (non-stable)].
    // Mount with loaderVersion='0.17.0-beta.1' (in the list, non-stable);
    // after load() resolves, the dropdown must still show that value,
    // not auto-flip to 0.16.0.
    const { findByLabelText } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'fabric', loaderVersion: '0.17.0-beta.1' },
    });
    const select = (await findByLabelText(/loader version/i)) as HTMLSelectElement;
    expect(select.value).toBe('0.17.0-beta.1');
  });

  it('auto-picks the stable entry when the parent passes a value not in the fetched list', async () => {
    // Companion to the above: a stale loaderVersion (e.g. user changed
    // MC and the previous loader-version is no longer compatible) must
    // still auto-pick stable rather than leave a broken-combo selection.
    const { findByLabelText } = render(LoaderPicker, {
      // 'nonexistent' is not in the mock list — must fall through to stable.
      props: { mc: '1.20.1', loader: 'fabric', loaderVersion: 'nonexistent-0.99' },
    });
    const select = (await findByLabelText(/loader version/i)) as HTMLSelectElement;
    expect(select.value).toBe('0.16.0');
  });
});
