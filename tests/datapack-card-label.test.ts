// The §6 state label: a datapack in the library is inert until placed into a
// world, so its catalog card must read «In library · vX» — never the bare
// `vX` ModCard renders for installed mods/assets, which is exactly the
// wrong-belief hazard (Modrinth App issue #4083) the slice-2 design names.
// The first draft targeted a string the card never renders; the REAL seam is
// the `installedLabel` prop threaded through ModResultsGrid — this test
// targets that prop.
import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { InstalledMod, ModSummary } from '$lib/ipc/bindings';
import ModCard from '$lib/mods/ModCard.svelte';

const summary: ModSummary = {
  source: 'modrinth',
  project_id: 'terralith',
  slug: 'terralith',
  name: 'Terralith',
  summary: 'World generation datapack',
  icon_url: null,
  downloads: 1000,
  author: 'Starmute',
  updated_at: null,
};

const installed: InstalledMod = {
  filename: 'Terralith_1.21_v2.5.5.zip',
  sha1: 'aa',
  source: 'modrinth',
  project_id: 'terralith',
  version_id: 'v1',
  name: 'Terralith',
  version_number: '2.5.5',
  installed_at: '2026-08-04T00:00:00Z',
  enabled: true,
  requires: [],
  enrich_attempted: false,
};

const noop = () => {};
const baseProps = {
  summary,
  installed,
  onInstall: noop,
  onOpenDetail: noop,
  onToggle: noop,
  onUninstall: noop,
  canToggle: false,
  layout: 'list' as const,
};

describe('ModCard installedLabel', () => {
  it('replaces the meta line with the supplied label', () => {
    render(ModCard, { props: { ...baseProps, installedLabel: 'In library · v2.5.5' } });
    expect(screen.getByText('In library · v2.5.5')).toBeTruthy();
    // The default `vX` meta must NOT also render — the label REPLACES it.
    expect(screen.queryByText('v2.5.5')).toBeNull();
  });

  it('keeps the default version meta when no label is supplied', () => {
    render(ModCard, { props: baseProps });
    expect(screen.getByText('v2.5.5')).toBeTruthy();
    expect(screen.queryByText(/In library/)).toBeNull();
  });
});
