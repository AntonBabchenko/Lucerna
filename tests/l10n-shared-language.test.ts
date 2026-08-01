// Pins the fix for the "modal says 81%, the Overview row says 100%" bug:
// the row and LocalizationModal must never be able to disagree about which
// language a translation percentage was measured against, because they now
// both render the SAME value passed in by the caller (+page.svelte's
// l10nLang) rather than each independently guessing or owning their own
// copy. This is a structural guarantee, not a sync-keeping one — so the
// test drives both components from one shared variable and asserts both
// surfaces reflect it, for more than one value.
import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstanceCoverage } from '$lib/ipc/bindings';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { modpacksCheckUpdates: vi.fn(), l10nCoverage: vi.fn() },
}));

import { commands } from '$lib/ipc/bindings';
import LocalizationModal from '$lib/l10n/LocalizationModal.svelte';
import { attentionCollapse } from '$lib/overview/attention-collapse.svelte';
import OverviewTab from '$lib/overview/OverviewTab.svelte';

function coverage(lang: string): InstanceCoverage {
  return {
    lang,
    percent: 81,
    namespaces: [],
    availableCodes: ['en_us', lang],
    applyGate: 'ready',
  };
}

const fabricInst = {
  id: 'i1',
  name: 'Skyblock',
  mc_version: '1.21.1',
  loader: 'fabric' as const,
  loader_version: '0.16.5',
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
  integrity: { healthy: true, checked_unix_ms: Date.now(), categories: [], problem_count: 0 },
  imported_from: null,
  created_from_server: null,
};

const overviewProps = {
  activeInstance: fabricInst,
  installedStats: { total: 18, enabled: 18, disabled: 0 },
  playtime: {
    total_seconds: 0,
    session_count: 0,
    last_session_seconds: 0,
    last_session_unix_ms: null,
  },
  incompatibleCount: 0,
  missingModsCount: 0,
  running: false,
  installing: false,
  exited: null,
  installError: null,
  modsError: null,
  errors: { listAccounts: null, remove: null, instances: null, versions: null },
  onManage: () => {},
  onExport: () => {},
  onOpenPackDrawer: () => {},
  onNavInstalled: () => {},
  onNavBrowse: () => {},
  onDismissError: () => {},
  onDismissInstallError: () => {},
  onDismissModsError: () => {},
  onOpenLogs: () => {},
  onOpenServers: () => {},
  onOpenLocalization: () => {},
  l10nPercent: 81,
};

beforeEach(() => {
  attentionCollapse.reset();
  vi.mocked(commands.l10nCoverage).mockClear();
});

describe('translation-target-language agreement', () => {
  it.each([
    'ru_ru',
    'de_de',
  ])('renders the same target language (%s) on the row and in the modal', async (lang) => {
    vi.mocked(commands.l10nCoverage).mockResolvedValue({
      status: 'ok',
      data: coverage(lang),
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);

    const { getByTestId } = render(OverviewTab, {
      props: { ...overviewProps, l10nLang: lang },
    });
    expect(getByTestId('overview-localization').textContent).toContain(lang);

    render(LocalizationModal, {
      props: { open: true, instanceId: fabricInst.id, lang },
    });
    const combo = await screen.findByRole('combobox');
    expect(combo.textContent).toContain(lang);
  });
});
