import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import CreateInstanceDialog from '$lib/instances/CreateInstanceDialog.svelte';
import type { VersionEntry } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  instanceMemoryBounds: vi.fn().mockResolvedValue({
    status: 'ok',
    data: {
      min_mb: 1024,
      max_mb: 8192,
      default_mb: 2048,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    },
  }),
  createInstance: vi.fn(),
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listForgeLoaders: m.listForgeLoaders,
    listFabricLoaders: m.listFabricLoaders,
    listQuiltLoaders: m.listQuiltLoaders,
    listNeoforgeLoaders: m.listNeoforgeLoaders,
    instanceMemoryBounds: m.instanceMemoryBounds,
    createInstance: m.createInstance,
  },
  events: {},
}));
// `dataLocation.fellBack` is a getter over the loaded status; the dialog only
// reads it, so a stub with a writable flag stands in for a recovery session.
const root = vi.hoisted(() => ({ fellBack: false }));
vi.mock('$lib/settings/data-location.svelte', () => ({
  dataLocation: {
    get fellBack() {
      return root.fellBack;
    },
  },
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: m.pushSuccess,
  pushWarning: m.pushWarning,
}));

const version: VersionEntry = {
  id: '1.20.1',
  version_type: 'release',
  release_date: '2024-01-01T00:00:00+00:00',
  url: '',
};

function mount(over: Record<string, unknown> = {}) {
  const props = {
    versions: [version],
    activate: vi.fn().mockResolvedValue(null),
    onCreated: vi.fn(),
    onClose: vi.fn(),
    ...over,
  };
  render(CreateInstanceDialog, { props });
  return props;
}

async function fillName(name = 'My Instance') {
  await fireEvent.input(screen.getByLabelText(/name/i), { target: { value: name } });
}
async function pickVersion() {
  await fireEvent.click(screen.getByRole('combobox'));
  await fireEvent.mouseDown(screen.getByRole('option', { name: '1.20.1' }));
}
const createButton = () => screen.getByTestId('create-instance-submit');
const errorText = () => screen.getByTestId('create-instance-error').textContent ?? '';

describe('CreateInstanceDialog', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    vi.clearAllMocks();
    root.fellBack = false;
    m.createInstance.mockResolvedValue({
      status: 'ok',
      data: { id: 'new-id', name: 'My Instance' },
    });
  });

  it('refuses an instance with no Minecraft version, whatever the loader, and says why', async () => {
    mount();
    await fillName();
    await fireEvent.click(createButton());
    expect(m.createInstance).not.toHaveBeenCalled();
    expect(errorText()).toContain('Pick a Minecraft version first');
  });

  it('says the reason once: beside the button, then in the alert after a click', async () => {
    mount();
    await fillName();
    const shown = () => screen.queryAllByText('Pick a Minecraft version first').length;
    expect(shown()).toBe(1);
    expect(errorText()).toBe('');
    await fireEvent.click(createButton());
    expect(shown()).toBe(1);
    expect(errorText()).toContain('Pick a Minecraft version first');
  });

  it('drops an announced reason once the form moves past it', async () => {
    mount();
    await fireEvent.click(createButton());
    expect(errorText()).toContain('Name is required');
    await fillName();
    expect(errorText()).toBe('');
    expect(screen.queryAllByText('Pick a Minecraft version first')).toHaveLength(1);
    // Its cause coming back does not bring the alert back unasked.
    await fillName('');
    expect(errorText()).toBe('');
    expect(screen.queryAllByText('Name is required')).toHaveLength(1);
  });

  it('creates once on a double click, then makes the new instance active and says so', async () => {
    let resolve!: (v: unknown) => void;
    m.createInstance.mockReturnValue(new Promise((r) => (resolve = r)));
    const p = mount();
    await fillName();
    await pickVersion();
    await fireEvent.click(createButton());
    await fireEvent.click(createButton());
    resolve({ status: 'ok', data: { id: 'new-id', name: 'My Instance' } });
    await waitFor(() => expect(p.onClose).toHaveBeenCalled());
    expect(m.createInstance).toHaveBeenCalledTimes(1);
    expect(m.createInstance.mock.calls[0].slice(0, 4)).toEqual([
      'My Instance',
      '1.20.1',
      'vanilla',
      null,
    ]);
    expect(p.activate).toHaveBeenCalledWith('new-id');
    expect(p.onCreated).toHaveBeenCalledWith('new-id');
    expect(m.pushSuccess).toHaveBeenCalledWith('Created My Instance and switched to it');
  });

  it('never claims a switch that failed', async () => {
    const p = mount({ activate: vi.fn().mockResolvedValue('active.json is locked') });
    await fillName();
    await pickVersion();
    await fireEvent.click(createButton());
    await waitFor(() => expect(p.onClose).toHaveBeenCalled());
    expect(m.pushSuccess).toHaveBeenCalledWith('Created My Instance');
    expect(m.pushWarning).toHaveBeenCalledWith("Couldn't switch to My Instance", [
      'active.json is locked',
    ]);
  });

  it('keeps the dialog open and names a refused create', async () => {
    m.createInstance.mockResolvedValue({ status: 'error', error: { kind: 'instance_name_empty' } });
    const p = mount();
    await fillName();
    await pickVersion();
    await fireEvent.click(createButton());
    await waitFor(() => expect(errorText()).toBeTruthy());
    expect(p.onClose).not.toHaveBeenCalled();
    expect(p.activate).not.toHaveBeenCalled();
  });

  it('says why nothing can be created while the data folder is unavailable', async () => {
    root.fellBack = true;
    mount();
    await fillName();
    await pickVersion();
    await fireEvent.click(createButton());
    expect(m.createInstance).not.toHaveBeenCalled();
    expect(errorText()).toBeTruthy();
  });
});

describe('CreateInstanceDialog — the version list says what it is', () => {
  beforeAll(() => locale.set('en'));

  it('a failed list shows the failure and Reload; a reload that succeeds hands the focus to the picker', async () => {
    const reload = vi.fn();
    const failed = {
      loading: false,
      failure:
        "Couldn't load the Minecraft version list. piston-meta.mojang.com is having problems.",
      reload,
    };
    const { rerender } = render(CreateInstanceDialog, {
      props: {
        versions: [],
        versionList: failed,
        activate: vi.fn().mockResolvedValue(null),
        onCreated: vi.fn(),
        onClose: vi.fn(),
      },
    });
    expect(screen.getByText(/Couldn't load the Minecraft version list/)).toBeTruthy();
    expect(screen.queryByText(/check your connection/)).toBeNull();
    const button = screen.getByRole('button', { name: 'Reload' });
    button.focus();
    await fireEvent.click(button);
    expect(reload).toHaveBeenCalledOnce();
    // While it reloads, the failure and its button stay (busy), so the focus is not dropped.
    await rerender({ versionList: { ...failed, loading: true } });
    // Busy, as BusyButton says it: the spinner's «Loading…» joins the name.
    expect(screen.getByRole('button', { name: /Reload/ }).getAttribute('aria-busy')).toBe('true');
    expect(document.activeElement).toBe(button);
    await rerender({
      versions: [version],
      versionList: { loading: false, failure: null, reload },
    });
    await waitFor(() => expect(document.activeElement?.id).toBe('create-mc-version'));
  });
});
