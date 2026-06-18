import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import LoaderPicker from '$lib/instances/LoaderPicker.svelte';

// Mock the IPC commands module so the component doesn't try to call
// real Tauri commands during unit tests. Fabric + Quilt mocks share
// the version "0.16.0" deliberately — the cross-loader regression
// test below pins that switching loaders never preserves a version
// just because the new loader's list happens to contain it.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listFabricLoaders: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        { version: '0.16.0', stable: true },
        { version: '0.17.0-beta.1', stable: false },
      ],
    }),
    listQuiltLoaders: vi.fn().mockResolvedValue({
      status: 'ok',
      data: [
        { version: '0.20.0', stable: true },
        { version: '0.16.0', stable: false },
      ],
    }),
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
    // The Select trigger carries id="loader-version-select"; the
    // <label for="loader-version-select"> associates the label text with
    // it. When the loader is vanilla the whole block is gone, so
    // queryByLabelText finds nothing.
    expect(queryByLabelText(/loader version/i)).toBeFalsy();
  });

  it('shows loader-version dropdown when a non-vanilla loader is selected and versions are loaded', async () => {
    const { getByText, findByLabelText } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'vanilla', loaderVersion: null },
    });
    await fireEvent.click(getByText('Fabric'));
    // The $effect re-fetches; the await findByLabelText polls until the
    // dropdown (the Select trigger <button>) materialises.
    const trigger = await findByLabelText(/loader version/i);
    expect(trigger).toBeTruthy();
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
    // after load() resolves, the Select trigger must still show that
    // value, not auto-flip to 0.16.0. The trigger renders the selected
    // option's label as its text content (non-stable label is the bare
    // version number).
    const { findByLabelText } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'fabric', loaderVersion: '0.17.0-beta.1' },
    });
    const trigger = (await findByLabelText(/loader version/i)) as HTMLElement;
    expect(trigger.textContent).toContain('0.17.0-beta.1');
  });

  it('auto-picks the stable entry when the parent passes a value not in the fetched list', async () => {
    // Companion to the above: a stale loaderVersion (e.g. user changed
    // MC and the previous loader-version is no longer compatible) must
    // still auto-pick stable rather than leave a broken-combo selection.
    const { findByLabelText } = render(LoaderPicker, {
      // 'nonexistent' is not in the mock list — must fall through to stable.
      props: { mc: '1.20.1', loader: 'fabric', loaderVersion: 'nonexistent-0.99' },
    });
    // Stable label renders as "{version} (recommended)" — contains 0.16.0.
    const trigger = (await findByLabelText(/loader version/i)) as HTMLElement;
    expect(trigger.textContent).toContain('0.16.0');
  });

  it('shows a spinner (role=status) while loader versions are loading, then shows the Select', async () => {
    const mod = await import('$lib/ipc/bindings');
    // Use a deferred promise so we can assert while the fetch is in flight.
    let resolveFabric!: (v: unknown) => void;
    const pendingFabric = new Promise((resolve) => {
      resolveFabric = resolve;
    });
    (mod.commands.listFabricLoaders as ReturnType<typeof vi.fn>).mockReturnValueOnce(pendingFabric);

    const { getByText, queryByRole, findByLabelText } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'vanilla', loaderVersion: null },
    });

    // Switch to Fabric — triggers the load, which is now pending.
    await fireEvent.click(getByText('Fabric'));

    // While the fetch is in flight the spinner should be visible.
    // The Spinner has delayMs=150 so we check immediately (before delay
    // fires) and after the delay — either way the status role appears once
    // the state is set. Since we use waitFor here we poll until it appears.
    await waitFor(() => {
      expect(queryByRole('status')).not.toBeNull();
    });

    // Resolve the fetch so the Select appears.
    resolveFabric({ status: 'ok', data: [{ version: '0.16.0', stable: true }] });
    const trigger = await findByLabelText(/loader version/i);
    expect(trigger).toBeTruthy();
  });

  it('resets to the new loader stable on switch, even when versions overlap', async () => {
    // Regression: a previous fix preserved loaderVersion across remount
    // by checking "is the current value in the fetched list" — but that
    // also preserved across loader SWITCHES when the two loaders shared
    // a version number. Concrete repro: pick Fabric → auto-picks 0.16.0
    // (Fabric's stable). Switch to Quilt → Quilt's list contains 0.16.0
    // (non-stable) AND 0.20.0 (stable). Previous fix kept 0.16.0; right
    // behavior is to reset to 0.20.0 — a loader switch is an explicit
    // user-driven ecosystem change, not a remount preservation case.
    const { getByText, findByLabelText } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'vanilla', loaderVersion: null },
    });

    await fireEvent.click(getByText('Fabric'));
    let trigger = (await findByLabelText(/loader version/i)) as HTMLElement;
    await waitFor(() => expect(trigger.textContent).toContain('0.16.0'));

    await fireEvent.click(getByText('Quilt'));
    // The {#if} re-creates the Select on loader switch, so the old
    // trigger ref can go stale — re-query before the second assertion.
    trigger = (await findByLabelText(/loader version/i)) as HTMLElement;
    await waitFor(() => {
      expect(trigger.textContent).toContain('0.20.0');
      expect(trigger.textContent).not.toContain('0.16.0');
    });
  });
});
