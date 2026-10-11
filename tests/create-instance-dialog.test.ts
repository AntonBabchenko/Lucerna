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
