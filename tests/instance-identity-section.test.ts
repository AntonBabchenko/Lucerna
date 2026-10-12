// The identity section of the instance Settings tab: picture, name, folder and
// the actions on the instance. The name tests moved here from
// manage-modal-tier1, the picture tests from manage-modal-instance-picture.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import InstanceIdentitySection from '$lib/instances/settings/InstanceIdentitySection.svelte';
import type { InstanceWithStatus } from '$lib/ipc/bindings';
import * as toasts from '$lib/toasts/toasts.svelte';

const m = vi.hoisted(() => ({
  instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
  previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
  renameInstanceDir: vi.fn(),
  setInstanceName: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
}));
vi.mock('$lib/ipc/bindings', () => ({ commands: { ...m }, events: {} }));
vi.mock('$lib/toasts/toasts.svelte', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/toasts/toasts.svelte')>();
  return { ...actual, pushSuccess: vi.fn(), pushWarning: vi.fn(), pushInfo: vi.fn() };
});
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('$lib/instances/instance-icon-cache', () => ({
  loadInstanceIcon: vi.fn().mockResolvedValue('data:image/png;base64,AAAA'),
  invalidateInstanceIcon: vi.fn(),
}));

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.1',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    min_heap_mb: null,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: true,
    has_icon: false,
    mrpack_name: null,
    mrpack_version: null,
    mrpack_project_id: null,
    mrpack_source: null,
    mrpack_summary: null,
    mrpack_version_id: null,
    integrity: null,
    imported_from: null,
    created_from_server: null,
    java: null,
    ...over,
  };
}

function props(over: Record<string, unknown> = {}) {
  return {
    instance: makeInstance(),
    running: false,
    busy: false,
    onChanged: vi.fn(),
    onClone: vi.fn(),
    onTranslations: vi.fn(),
    onOpenFolder: vi.fn(),
    ...over,
  };
}

describe('InstanceIdentitySection — the name', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => vi.clearAllMocks());

  it('keeps an in-progress edit when the instance refreshes with the same id', async () => {
    const p = props();
    const { rerender } = render(InstanceIdentitySection, { props: p });
    const input = (await screen.findByDisplayValue('Default')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'My New Name' } });
    await rerender({ ...p, instance: makeInstance({ name: 'Default' }) });
    expect(screen.getByDisplayValue('My New Name')).toBeTruthy();
  });

  it('resets the draft when another instance becomes the shown one', async () => {
    const p = props({ instance: makeInstance({ id: 'a', name: 'Alpha' }) });
    const { rerender } = render(InstanceIdentitySection, { props: p });
    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'edited-but-not-committed' } });
    await rerender({ ...p, instance: makeInstance({ id: 'b', name: 'Beta' }) });
    expect(screen.getByDisplayValue('Beta')).toBeTruthy();
  });

  it('saves on blur with a transient Saved badge, hidden again on re-edit', async () => {
    render(InstanceIdentitySection, {
      props: props({ instance: makeInstance({ name: 'Alpha' }) }),
    });
    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'Renamed' } });
    await fireEvent.blur(input);
    expect(m.setInstanceName).toHaveBeenCalledWith('inst-1', 'Renamed');
    expect(await screen.findByText('Saved')).toBeTruthy();
    await fireEvent.input(input, { target: { value: 'Renamed2' } });
    expect(screen.queryByText('Saved')).toBeNull();
  });

  it('reverts a blanked name to the saved value without writing', async () => {
    render(InstanceIdentitySection, {
      props: props({ instance: makeInstance({ name: 'Alpha' }) }),
    });
    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: '   ' } });
    await fireEvent.blur(input);
    expect(await screen.findByDisplayValue('Alpha')).toBeTruthy();
    expect(m.setInstanceName).not.toHaveBeenCalled();
  });

  it('drives the name counter and maxlength from a single constant', async () => {
    render(InstanceIdentitySection, {
      props: props({ instance: makeInstance({ name: 'Alpha' }) }),
    });
    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    expect(input.getAttribute('maxlength')).toBe('32');
    expect(screen.getByText('5/32')).toBeTruthy();
  });

  it('surfaces a refused name in an alert next to it', async () => {
    m.setInstanceName.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'instance_name_empty' },
    });
    render(InstanceIdentitySection, {
      props: props({ instance: makeInstance({ name: 'Alpha' }) }),
    });
    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'Renamed' } });
    await fireEvent.blur(input);
    const msg = await screen.findByText(/name cannot be empty/i);
    expect(msg.closest('[role="alert"]')).not.toBeNull();
  });

  it('a name save that fails after the tab is gone is a toast naming the instance', async () => {
    let resolve!: (v: unknown) => void;
    m.setInstanceName.mockReturnValueOnce(new Promise((r) => (resolve = r)));
    const { unmount } = render(InstanceIdentitySection, { props: props() });
    const input = (await screen.findByDisplayValue('Default')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'Renamed' } });
    await fireEvent.blur(input);
    unmount();
    resolve({ status: 'error', error: { kind: 'instance_name_empty' } });
    await waitFor(() => expect(vi.mocked(toasts.pushWarning)).toHaveBeenCalled());
    expect(vi.mocked(toasts.pushWarning).mock.calls[0][0]).toBe("Name of Default wasn't saved");
  });

  it('Rename lands in the name field with the whole name selected', async () => {
    const onFocusDelivered = vi.fn();
    render(InstanceIdentitySection, {
      props: props({
        instance: makeInstance({ name: 'Alpha' }),
        focusField: 'name',
        onFocusDelivered,
      }),
    });
    await waitFor(() => expect(onFocusDelivered).toHaveBeenCalled());
    const input = screen.getByDisplayValue('Alpha') as HTMLInputElement;
    await waitFor(() => expect(document.activeElement).toBe(input));
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe('Alpha'.length);
  });
});

describe('InstanceIdentitySection — folder and actions', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => vi.clearAllMocks());

  it('follows a folder rename: the draft survives the new id', async () => {
    const before = makeInstance({ id: 'Old-Name', name: 'My Pack' });
    const after = makeInstance({ id: 'New-Name', name: 'My Pack' });
    m.renameInstanceDir.mockResolvedValue({ status: 'ok', data: after });
    const p = props({ instance: before });
    const { rerender } = render(InstanceIdentitySection, { props: p });
    const input = (await screen.findByDisplayValue('My Pack')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'My Pack 2' } });

    await fireEvent.click(await screen.findByRole('button', { name: 'Change' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Rename' }));
    await waitFor(() => expect(p.onChanged).toHaveBeenCalled());
    // The page's refresh brings the renamed instance in as the active one.
    await rerender({ ...p, instance: after });
    expect(screen.getByDisplayValue('My Pack 2')).toBeTruthy();
  });

  it('locks the folder rename with a visible reason while the game runs or a task runs', () => {
    const { unmount } = render(InstanceIdentitySection, { props: props({ running: true }) });
    expect((screen.getByRole('button', { name: 'Change' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    expect(screen.getByText(/folder can be renamed once/i)).toBeTruthy();
    unmount();
    render(InstanceIdentitySection, { props: props({ busy: true }) });
    expect((screen.getByRole('button', { name: 'Change' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it('the actions act on the shown instance; Clone waits for the game to stop', async () => {
    const p = props({ onShortcut: vi.fn() });
    const { unmount } = render(InstanceIdentitySection, { props: p });
    await fireEvent.click(screen.getByTestId('clone-instance-btn'));
    await fireEvent.click(screen.getByTestId('create-shortcut-btn'));
    await fireEvent.click(screen.getByTestId('manage-translations-btn'));
    await fireEvent.click(screen.getByTestId('settings-open-folder-btn'));
    expect(p.onClone).toHaveBeenCalledWith('inst-1');
    expect(p.onShortcut).toHaveBeenCalledWith('inst-1');
    expect(p.onTranslations).toHaveBeenCalledWith('inst-1');
    expect(p.onOpenFolder).toHaveBeenCalledWith('inst-1');
    const row = screen
      .getByTestId('clone-instance-btn')
      .closest('[data-tour-ctx="settings-actions"]');
    expect(row?.className).toContain('flex-wrap');
    unmount();
    render(InstanceIdentitySection, { props: props({ running: true }) });
    expect((screen.getByTestId('clone-instance-btn') as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByTestId('create-shortcut-btn')).toBeNull();
  });

  it("shows the instance's picture, and the letter avatar without one", async () => {
    const { container, unmount } = render(InstanceIdentitySection, {
      props: props({ instance: makeInstance({ has_icon: true }) }),
    });
    await waitFor(() => expect(container.querySelectorAll('img').length).toBe(1));
    unmount();
    const second = render(InstanceIdentitySection, { props: props() });
    await waitFor(() => expect(second.container.querySelector('#detail-name')).toBeTruthy());
    expect(second.container.querySelectorAll('img').length).toBe(0);
  });
});
