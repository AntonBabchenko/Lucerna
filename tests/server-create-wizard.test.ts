import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';
import ServerCreateWizard from '$lib/servers/ServerCreateWizard.svelte';

// Hoisted so the vi.mock factory below can reference it.
const { pushSuccessMock, serverCoreVersionsMock } = vi.hoisted(() => ({
  pushSuccessMock: vi.fn(),
  serverCoreVersionsMock: vi.fn(),
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: pushSuccessMock,
  pushError: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // ServerCreated shape: { server, quarantined }. Two client mods were set
    // aside, so a successful create fires the quarantine-summary toast.
    serverCreate: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        server: { id: 'srv-1' },
        quarantined: ['betterf3.jar.disabled', 'oculus.jar.disabled'],
      },
    }),
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    // LoaderPicker fires these when ServerImportView's confirm step renders.
    listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // Plugin-core catalogue: the ServerCorePicker/wizard call this when Paper
    // or Purpur is selected. Controlled per-test (resolve subset / pending /
    // error) via serverCoreVersionsMock.
    serverCoreVersions: serverCoreVersionsMock,
  },
}));

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    refresh: vi.fn().mockResolvedValue(undefined),
    importInspect: vi.fn(),
    importCommit: vi.fn().mockResolvedValue({ ok: true }),
    importCancel: vi.fn().mockResolvedValue(undefined),
  },
}));

// Stub the Tauri dialog so "Choose .zip" / "Choose folder" resolve without hanging.
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue(null),
}));

const mockInstance: InstanceWithStatus = {
  id: 'inst-1',
  name: 'My Instance',
  mc_version: '1.21.1',
  loader: 'fabric',
  loader_version: '0.16.0',
  max_heap_mb: 4096,
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
};

function baseProps(overrides: Record<string, unknown> = {}) {
  return {
    instances: [mockInstance],
    versions: [],
    onDone: vi.fn(),
    onCancel: vi.fn(),
    ...overrides,
  };
}

// Three release versions for the standalone MC-version dropdown. Paper's
// catalogue below covers only two of them, so the intersection is observable.
function mkVersion(id: string): VersionEntry {
  return {
    id,
    version_type: 'release',
    release_date: '2024-01-01T00:00:00+00:00',
    url: `https://example.invalid/${id}.json`,
  };
}
const RELEASES: VersionEntry[] = [mkVersion('1.21.1'), mkVersion('1.20.6'), mkVersion('1.19.4')];

// Enter standalone mode and select a core by its display label (e.g. 'Paper').
// The mode toggle button is 'From scratch'; core buttons are labelled by
// displayCore. Both are plain <button>s.
async function pickStandaloneCore(coreLabel: string): Promise<void> {
  // The mode-toggle button has no aria-label; its accessible name is the
  // concatenation of its title + hint spans ("From scratch Pick version …").
  await fireEvent.click(screen.getByRole('button', { name: /^From scratch/ }));
  await fireEvent.click(screen.getByRole('button', { name: coreLabel }));
}

// The custom MC-version Select renders as role=combobox (aria-label 'Minecraft
// version'); its options are role=option and commit on mousedown. The trigger
// toggles the popover, so these helpers key off aria-expanded to stay
// idempotent (safe to call repeatedly inside vi.waitFor).
function mcVersionCombobox(): HTMLElement {
  return screen.getByRole('combobox', { name: 'Minecraft version' });
}
async function ensureMcListOpen(): Promise<void> {
  const box = mcVersionCombobox();
  if (box.getAttribute('aria-expanded') !== 'true') await fireEvent.click(box);
}
async function ensureMcListClosed(): Promise<void> {
  const box = mcVersionCombobox();
  if (box.getAttribute('aria-expanded') === 'true') await fireEvent.click(box);
}
// Open → read labels → close, so callers get a clean state each invocation.
async function readMcVersionOptions(): Promise<string[]> {
  await ensureMcListOpen();
  const labels = within(screen.getByRole('listbox'))
    .getAllByRole('option')
    .map((o) => o.textContent?.trim() ?? '');
  await ensureMcListClosed();
  return labels;
}
async function pickMcVersion(id: string): Promise<void> {
  await ensureMcListOpen();
  const option = within(screen.getByRole('listbox'))
    .getAllByRole('option')
    .find((o) => o.textContent?.trim() === id);
  if (!option) throw new Error(`MC version option "${id}" not offered`);
  await fireEvent.mouseDown(option); // commit closes the list
}

describe('ServerCreateWizard', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    // Default: Paper/Purpur publish two of the three releases. Individual
    // tests override this (pending / error) before rendering.
    serverCoreVersionsMock.mockReset();
    serverCoreVersionsMock.mockResolvedValue({ status: 'ok', data: ['1.21.1', '1.20.6'] });
  });

  it('Create button is disabled when EULA is unchecked even with a name filled', async () => {
    render(ServerCreateWizard, baseProps());

    // Fill in name
    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    await fireEvent.input(nameInput, { target: { value: 'Test Server' } });

    // EULA is unchecked by default — Create must remain disabled
    const createBtn = screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement;
    expect(createBtn.disabled).toBe(true);
  });

  it('Create button becomes enabled after checking EULA with name present', async () => {
    render(ServerCreateWizard, baseProps());

    // Fill in name
    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    await fireEvent.input(nameInput, { target: { value: 'Test Server' } });

    // Check the EULA checkbox
    const eulaCheckbox = screen.getByRole('checkbox') as HTMLInputElement;
    await fireEvent.click(eulaCheckbox);

    // Create should now be enabled
    const createBtn = screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement;
    expect(createBtn.disabled).toBe(false);
  });

  it('names why Create is disabled and clears the reason once satisfied (#21-FE)', async () => {
    render(ServerCreateWizard, baseProps());

    // Empty name → the name requirement is surfaced.
    expect(screen.getByTestId('wizard-disabled-reason').textContent).toContain('Enter a name');

    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    await fireEvent.input(nameInput, { target: { value: 'Test Server' } });

    // Name ok + instance preselected → only the EULA remains.
    expect(screen.getByTestId('wizard-disabled-reason').textContent).toContain(
      'Accept the Minecraft EULA',
    );

    await fireEvent.click(screen.getByRole('checkbox'));

    // Everything satisfied → no reason is shown.
    expect(screen.queryByTestId('wizard-disabled-reason')).toBeNull();
  });

  it('switching to Import mode shows the import source pickers', async () => {
    render(ServerCreateWizard, baseProps());
    await fireEvent.click(screen.getByRole('button', { name: 'Import existing' }));
    expect(screen.getByRole('button', { name: 'Choose .zip' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Choose folder' })).toBeTruthy();
  });

  it('finishes and shows a quarantine summary toast when client mods are set aside', async () => {
    pushSuccessMock.mockClear();
    const onDone = vi.fn();
    render(ServerCreateWizard, baseProps({ onDone }));

    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    await fireEvent.input(nameInput, { target: { value: 'Test Server' } });
    const eulaCheckbox = screen.getByRole('checkbox') as HTMLInputElement;
    await fireEvent.click(eulaCheckbox);
    await fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    // The async create handler refreshes, toasts the summary (2 mods set aside),
    // then calls onDone.
    await vi.waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(pushSuccessMock).toHaveBeenCalledTimes(1);
  });

  // ── Paper/Purpur core-version intersection (Task 18) ──────────────────────

  it('narrows the MC-version dropdown to the core-supported set when Paper is selected', async () => {
    render(ServerCreateWizard, baseProps({ versions: RELEASES }));

    // Vanilla (default core): all three releases are offered (+ placeholder).
    await pickStandaloneCore('Vanilla');
    const vanillaOptions = await readMcVersionOptions();
    expect(vanillaOptions).toEqual(expect.arrayContaining(['1.21.1', '1.20.6', '1.19.4']));

    // Switch to Paper: the catalogue (1.21.1, 1.20.6) filters out 1.19.4.
    await pickStandaloneCore('Paper');
    await vi.waitFor(() => expect(serverCoreVersionsMock).toHaveBeenCalledWith('paper'));

    const paperOptions = await vi.waitFor(async () => {
      const opts = await readMcVersionOptions();
      // The list must have narrowed before we assert on it.
      if (opts.includes('1.19.4')) throw new Error('dropdown not yet filtered');
      return opts;
    });
    expect(paperOptions).toContain('1.21.1');
    expect(paperOptions).toContain('1.20.6');
    expect(paperOptions).not.toContain('1.19.4');
  });

  it('blocks Create while the Paper catalogue loads, on error, and enables it once satisfied', async () => {
    // Catalogue never resolves → the wizard stays in the loading state.
    let resolveCatalogue: (v: { status: 'ok'; data: string[] }) => void = () => {};
    serverCoreVersionsMock.mockReset();
    serverCoreVersionsMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveCatalogue = resolve;
        }),
    );

    render(ServerCreateWizard, baseProps({ versions: RELEASES }));
    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    await fireEvent.input(nameInput, { target: { value: 'My Paper Server' } });
    await fireEvent.click(screen.getByRole('checkbox')); // accept EULA
    await pickStandaloneCore('Paper');

    // Loading: the blocked reason names the catalogue fetch, not "loader version".
    await vi.waitFor(() =>
      expect(screen.getByTestId('wizard-disabled-reason').textContent).toContain(
        'Checking available versions',
      ),
    );
    expect((screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement).disabled).toBe(
      true,
    );

    // Resolve the catalogue and pick a supported MC version → Create enables.
    resolveCatalogue({ status: 'ok', data: ['1.21.1', '1.20.6'] });
    await vi.waitFor(async () => {
      const opts = await readMcVersionOptions();
      if (opts.includes('1.19.4')) throw new Error('dropdown not yet filtered');
    });
    await pickMcVersion('1.21.1');
    await vi.waitFor(() =>
      expect((screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement).disabled).toBe(
        false,
      ),
    );
  });

  it('blocks Create with the fetch error when the Paper catalogue fails to load', async () => {
    serverCoreVersionsMock.mockReset();
    serverCoreVersionsMock.mockResolvedValue({ status: 'error', error: { kind: 'network' } });

    render(ServerCreateWizard, baseProps({ versions: RELEASES }));
    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    await fireEvent.input(nameInput, { target: { value: 'My Paper Server' } });
    await fireEvent.click(screen.getByRole('checkbox'));
    await pickStandaloneCore('Paper');

    // The blocked reason surfaces the formatted fetch error, not a loader prompt.
    await vi.waitFor(() =>
      expect(screen.getByTestId('wizard-disabled-reason').textContent).toContain(
        "Couldn't reach the server",
      ),
    );
    expect((screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it('auto-clears an MC version the newly-chosen core does not support', async () => {
    // Paper covers 1.21.1 + 1.20.6 but NOT 1.19.4.
    render(ServerCreateWizard, baseProps({ versions: RELEASES }));

    // Fill name + EULA so the disabled-reason we assert on later isn't masked
    // by the earlier name/EULA checks.
    await fireEvent.input(screen.getByLabelText('Name') as HTMLInputElement, {
      target: { value: 'My Server' },
    });
    await fireEvent.click(screen.getByRole('checkbox'));

    // Under Vanilla, pick 1.19.4 (a version Paper lacks).
    await pickStandaloneCore('Vanilla');
    await pickMcVersion('1.19.4');
    expect(mcVersionCombobox().textContent).toContain('1.19.4');

    // Switch to Paper: 1.19.4 is out of the intersection → auto-cleared to the
    // placeholder, and Create is blocked with the pick-a-version reason.
    await pickStandaloneCore('Paper');
    await vi.waitFor(() =>
      expect(mcVersionCombobox().textContent).toContain('-- Choose Minecraft version --'),
    );
    expect(mcVersionCombobox().textContent).not.toContain('1.19.4');
    expect(screen.getByTestId('wizard-disabled-reason').textContent).toContain(
      'Choose a Minecraft version',
    );
  });
});
