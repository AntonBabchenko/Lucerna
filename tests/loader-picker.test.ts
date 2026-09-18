import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
import LoaderPickerBound from './fixtures/LoaderPickerBound.svelte';

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

  it('announces a loader-load failure in a live region (role=alert)', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.listFabricLoaders as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'loader_unavailable', loader: 'fabric', mc_version: '1.20.1' },
    });

    const { getByText, getByRole } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'vanilla', loaderVersion: null },
    });
    await fireEvent.click(getByText('Fabric'));

    // The persistent StatusMessage region is always role=alert; wait for the
    // localized error text to populate it.
    await waitFor(() => expect(getByRole('alert').textContent).toMatch(/does not support/i));
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

// A macrotask drain: covers the whole microtask queue in one wait, so a
// (wrong) extra request has had every chance to fire before we count.
const flush = () => new Promise((r) => setTimeout(r, 0));

const pressed = (el: HTMLElement) => el.getAttribute('aria-pressed');

// With `onchange` the picker is in COMMIT MODE: the props are the parent's
// committed truth, `onchange` is a request to change it, and its verdict
// decides whether the clicked value stays on screen. The bug this pins: the
// picker wrote the clicked loader into its own (one-way) prop before the parent
// answered, so a cancelled pack-detach prompt or a failed write left the picker
// showing a loader the instance did not have — and the request itself went out
// twice, the first time carrying the PREVIOUS ecosystem's version.
describe('LoaderPicker — commit requests (onchange)', () => {
  const committed = { mc: '1.20.1', loader: 'fabric' as const, loaderVersion: '0.17.0-beta.1' };

  it('a loader click requests exactly one commit, with the clicked ecosystem stable version', async () => {
    const onchange = vi.fn().mockResolvedValue(true);
    const { getByRole, findByLabelText } = render(LoaderPicker, {
      props: { ...committed, onchange },
    });
    // Mount: the committed version is a real build → nothing to request.
    await findByLabelText(/loader version/i);
    await flush();
    expect(onchange).not.toHaveBeenCalled();

    await fireEvent.click(getByRole('button', { name: 'Quilt' }));
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    await flush();
    expect(onchange).toHaveBeenCalledTimes(1);
    expect(onchange).toHaveBeenCalledWith('quilt', '0.20.0');
  });

  it('a refused commit puts the pressed loader and the version back to the props', async () => {
    const onchange = vi.fn().mockResolvedValue(false);
    const { getByRole, getByLabelText, findByLabelText } = render(LoaderPicker, {
      props: { ...committed, onchange },
    });
    await findByLabelText(/loader version/i);

    await fireEvent.click(getByRole('button', { name: 'Quilt' }));
    await waitFor(() => expect(onchange).toHaveBeenCalledTimes(1));

    await waitFor(() => {
      expect(pressed(getByRole('button', { name: 'Fabric' }))).toBe('true');
      expect(pressed(getByRole('button', { name: 'Quilt' }))).toBe('false');
    });
    await waitFor(() =>
      expect(getByLabelText(/loader version/i).textContent).toContain('0.17.0-beta.1'),
    );
  });

  it('an onchange that throws is a refusal', async () => {
    const onchange = vi.fn(() => {
      throw new Error('handler blew up');
    });
    const { getByRole, findByLabelText } = render(LoaderPicker, {
      props: { ...committed, onchange },
    });
    await findByLabelText(/loader version/i);

    await fireEvent.click(getByRole('button', { name: 'Quilt' }));
    await waitFor(() => expect(onchange).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(pressed(getByRole('button', { name: 'Fabric' }))).toBe('true'));
  });

  it('requests nothing when the clicked loader list fails', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.listQuiltLoaders as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'loader_unavailable', loader: 'quilt', mc_version: '1.20.1' },
    });
    const onchange = vi.fn().mockResolvedValue(true);
    const { getByRole, findByLabelText } = render(LoaderPicker, {
      props: { ...committed, onchange },
    });
    await findByLabelText(/loader version/i);

    await fireEvent.click(getByRole('button', { name: 'Quilt' }));
    await waitFor(() => expect(getByRole('alert').textContent).toMatch(/does not support/i));
    await flush();
    // No valid (loader, version) pair exists, so there is nothing to write.
    expect(onchange).not.toHaveBeenCalled();
  });

  it('an empty list is reported as unavailable and requests nothing', async () => {
    const onchange = vi.fn().mockResolvedValue(true);
    const { getByRole, findByLabelText } = render(LoaderPicker, {
      props: { ...committed, onchange },
    });
    await findByLabelText(/loader version/i);

    // The Forge mock answers `ok` with an empty list.
    await fireEvent.click(getByRole('button', { name: 'Forge' }));
    await waitFor(() => expect(getByRole('alert').textContent).toMatch(/does not support/i));
    await flush();
    expect(onchange).not.toHaveBeenCalled();
  });

  it('after a failed list, clicking the committed loader abandons the draft without a request', async () => {
    const mod = await import('$lib/ipc/bindings');
    (mod.commands.listQuiltLoaders as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'loader_unavailable', loader: 'quilt', mc_version: '1.20.1' },
    });
    const onchange = vi.fn().mockResolvedValue(true);
    const { getByRole, getByLabelText, findByLabelText } = render(LoaderPicker, {
      props: { ...committed, onchange },
    });
    await findByLabelText(/loader version/i);
    await fireEvent.click(getByRole('button', { name: 'Quilt' }));
    await waitFor(() => expect(getByRole('alert').textContent).toMatch(/does not support/i));

    await fireEvent.click(getByRole('button', { name: 'Fabric' }));
    await waitFor(() => expect(pressed(getByRole('button', { name: 'Fabric' }))).toBe('true'));
    // The pinned NON-stable version survives: going back is not a "switch".
    await waitFor(() =>
      expect(getByLabelText(/loader version/i).textContent).toContain('0.17.0-beta.1'),
    );
    await flush();
    expect(onchange).not.toHaveBeenCalled();
    expect(getByRole('alert').textContent).toBe('');
  });

  it('a parent-driven loader change keeps the version the parent passed and requests nothing', async () => {
    // A pack restore writes the pack's own loader back and the parent refetches:
    // that is not the user switching ecosystems, so nothing may be reset to
    // "recommended" and committed over it. Quilt's list holds 0.16.0 as a
    // NON-stable build — a (wrong) reset would land on 0.20.0.
    const onchange = vi.fn().mockResolvedValue(true);
    const { rerender, getByLabelText, findByLabelText } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'fabric', loaderVersion: '0.16.0', onchange },
    });
    await findByLabelText(/loader version/i);

    await rerender({ mc: '1.20.1', loader: 'quilt', loaderVersion: '0.16.0', onchange });
    await waitFor(() => {
      const text = getByLabelText(/loader version/i).textContent ?? '';
      expect(text).toContain('0.16.0');
      expect(text).not.toContain('0.20.0');
    });
    await flush();
    expect(onchange).not.toHaveBeenCalled();
  });

  it('a late refusal from an overtaken pick does not revert the newer pick', async () => {
    let answerFirst!: (accepted: boolean) => void;
    const onchange = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise<boolean>((resolve) => {
            answerFirst = resolve;
          }),
      )
      .mockResolvedValue(true);
    const { getByRole, findByLabelText } = render(LoaderPicker, {
      props: { ...committed, onchange },
    });
    await findByLabelText(/loader version/i);

    await fireEvent.click(getByRole('button', { name: 'Quilt' }));
    await waitFor(() => expect(onchange).toHaveBeenCalledTimes(1));
    await fireEvent.click(getByRole('button', { name: 'Vanilla' }));
    await waitFor(() => expect(onchange).toHaveBeenCalledTimes(2));
    expect(onchange).toHaveBeenLastCalledWith('vanilla', null);

    answerFirst(false);
    await flush();
    expect(pressed(getByRole('button', { name: 'Vanilla' }))).toBe('true');
  });

  it('while a commit is in flight, clicking the committed loader requests the committed pair', async () => {
    // Last click wins: the Quilt write may still land, so "back to Fabric" has
    // to be a real request — and for the pinned version, not for "recommended".
    const onchange = vi.fn(() => new Promise<boolean>(() => {}));
    const { getByRole, findByLabelText } = render(LoaderPicker, {
      props: { ...committed, onchange },
    });
    await findByLabelText(/loader version/i);

    await fireEvent.click(getByRole('button', { name: 'Quilt' }));
    await waitFor(() => expect(onchange).toHaveBeenCalledTimes(1));
    await fireEvent.click(getByRole('button', { name: 'Fabric' }));
    await waitFor(() => expect(onchange).toHaveBeenCalledTimes(2));
    expect(onchange).toHaveBeenLastCalledWith('fabric', '0.17.0-beta.1');
  });

  it('a version pick is a commit request, and a refusal restores the committed version', async () => {
    const onchange = vi.fn().mockResolvedValue(false);
    const { getByRole, getByLabelText, findByLabelText } = render(LoaderPicker, {
      props: { mc: '1.20.1', loader: 'fabric', loaderVersion: '0.16.0', onchange },
    });
    const trigger = await findByLabelText(/loader version/i);

    await fireEvent.click(trigger);
    await fireEvent.mouseDown(getByRole('option', { name: '0.17.0-beta.1' }));
    await waitFor(() => expect(onchange).toHaveBeenCalledWith('fabric', '0.17.0-beta.1'));
    await waitFor(() => expect(getByLabelText(/loader version/i).textContent).toContain('0.16.0'));
  });
});

// Without `onchange` (the create form) the picker edits the parent's draft
// through `bind:`. Nothing can refuse a draft edit, but the draft must never
// hold a cross-ecosystem pair: the create form enables Create whenever a
// version is present, so (quilt, <a Fabric version>) was creatable for as long
// as the Quilt list took to load.
describe('LoaderPicker — bound draft (no onchange)', () => {
  it('a loader click clears the bound version until the new list resolves', async () => {
    const mod = await import('$lib/ipc/bindings');
    let resolveQuilt!: (v: unknown) => void;
    (mod.commands.listQuiltLoaders as ReturnType<typeof vi.fn>).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveQuilt = resolve;
      }),
    );
    const { getByRole, getByTestId, findByLabelText } = render(LoaderPickerBound);
    await findByLabelText(/loader version/i);
    expect(getByTestId('bound').textContent).toBe('fabric|0.16.0');

    await fireEvent.click(getByRole('button', { name: 'Quilt' }));
    await waitFor(() => expect(getByTestId('bound').textContent).toBe('quilt|null'));

    resolveQuilt({ status: 'ok', data: [{ version: '0.20.0', stable: true }] });
    await waitFor(() => expect(getByTestId('bound').textContent).toBe('quilt|0.20.0'));
  });
});
