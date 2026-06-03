import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import InstalledModRow from '$lib/mods/installed/InstalledModRow.svelte';

const summary = {
  source: 'modrinth' as const,
  project_id: 'p',
  slug: 's',
  name: 'Alpha',
  summary: '',
  icon_url: null,
  downloads: 1,
  author: 'x',
  updated_at: null,
};
const installed = (enabled: boolean) => ({
  filename: 'a.jar',
  sha1: 'a',
  source: 'modrinth' as const,
  project_id: 'p',
  version_id: 'v',
  name: 'Alpha',
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled,
  enrich_attempted: false,
});
const base = () => ({
  summary,
  rowKey: 'modrinth:p',
  root: undefined,
  requiredByNames: [] as string[],
  depTotal: 0,
  depMissing: 0,
  expanded: false,
  graphLoading: false,
  hoveredKey: null,
  updateState: null,
  checking: false,
  packChip: null,
  selected: false,
  onToggleExpand() {},
  onHover() {},
  onOpenDetail() {},
  onToggle() {},
  onUninstall() {},
  onUpdate() {},
  onSelectChange() {},
  onInstallDep() {},
  onJump() {},
});

describe('status badge priority', () => {
  it('shows "missing" when depMissing > 0 even if disabled', () => {
    render(InstalledModRow, { props: { ...base(), installed: installed(false), depMissing: 2 } });
    expect(screen.getByTestId('status-badge').textContent).toMatch(/2 missing/);
  });
  it('shows update arrow when an update is available and no missing deps', () => {
    render(InstalledModRow, {
      props: {
        ...base(),
        installed: installed(true),
        updateState: { kind: 'update_available', target: { version_number: '2.0' } } as never,
      },
    });
    expect(screen.getByTestId('status-badge').textContent).toMatch(/↑ 2\.0/);
  });
  it('shows "off" when disabled with no missing deps or update', () => {
    render(InstalledModRow, { props: { ...base(), installed: installed(false) } });
    expect(screen.getByTestId('status-badge').textContent).toMatch(/off/);
  });
  it('shows no badge when enabled, no updates, no missing deps', () => {
    render(InstalledModRow, { props: { ...base(), installed: installed(true) } });
    expect(screen.queryByTestId('status-badge')).toBeNull();
  });
});
