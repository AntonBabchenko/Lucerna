import { fireEvent, render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import ImportedCard from '$lib/modpacks/ImportedCard.svelte';
import { modpackUpdates } from '$lib/modpacks/modpack-updates.svelte';

const baseInst = {
  id: 'i1',
  name: 'My Instance',
  mc_version: '1.20.1',
  loader: 'fabric' as const,
  loader_version: '0.15.7',
  max_heap_mb: 2048,
  extra_jvm_args: '',
  created_unix_ms: Date.now() - 24 * 60 * 60 * 1000, // ~1 day ago
  ready: true,
  mrpack_name: 'Fabulously Optimized',
  mrpack_version: '5.9.0',
  mrpack_project_id: null,
  mrpack_source: null,
  mrpack_summary: null,
  mrpack_version_id: null,
  integrity: null,
};

describe('ImportedCard', () => {
  it('renders pack title and version', () => {
    const { getByText } = render(ImportedCard, {
      props: { inst: baseInst, onClick: () => {} },
    });
    expect(getByText(/Fabulously Optimized/)).toBeTruthy();
    expect(getByText(/v5\.9\.0/)).toBeTruthy();
  });

  it('renders instance name as sub-line', () => {
    const { getByText } = render(ImportedCard, {
      props: { inst: baseInst, onClick: () => {} },
    });
    expect(getByText(/Instance: My Instance/)).toBeTruthy();
  });

  it('renders MC version and loader in meta line', () => {
    const { getByText } = render(ImportedCard, {
      props: { inst: baseInst, onClick: () => {} },
    });
    // The meta line is a single textContent; substring-match avoids
    // brittle whitespace/template assertions.
    expect(getByText(/MC 1\.20\.1/)).toBeTruthy();
    expect(getByText(/Fabric/)).toBeTruthy();
  });

  it('fires onClick when card is clicked', async () => {
    const onClick = vi.fn();
    const { getByTestId } = render(ImportedCard, {
      props: { inst: baseInst, onClick },
    });
    await fireEvent.click(getByTestId('imported-card'));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('shows a package placeholder icon when no pack icon URL is stored', () => {
    const { container } = render(ImportedCard, {
      props: { inst: baseInst, onClick: () => {} },
    });
    expect(container.querySelector('.lucide-package')).toBeTruthy();
  });

  it('renders without loader_version when not set', () => {
    const inst = { ...baseInst, loader: 'vanilla' as const, loader_version: null };
    const { getByText } = render(ImportedCard, {
      props: { inst, onClick: () => {} },
    });
    // Vanilla rendered via displayLoader; no version suffix expected.
    expect(getByText(/Vanilla/)).toBeTruthy();
  });

  // Bundle 2 — modified tag in the title row.

  it("renders 'modified' tag when isModified is true", () => {
    const { getByTestId } = render(ImportedCard, {
      props: { inst: baseInst, onClick: () => {}, isModified: true },
    });
    const tag = getByTestId('imported-card-modified-tag');
    expect(tag.textContent).toContain('modified');
  });

  it("hides 'modified' tag when isModified is false", () => {
    const { queryByTestId } = render(ImportedCard, {
      props: { inst: baseInst, onClick: () => {}, isModified: false },
    });
    expect(queryByTestId('imported-card-modified-tag')).toBeNull();
  });

  it("hides 'modified' tag when isModified is not passed (default false)", () => {
    const { queryByTestId } = render(ImportedCard, {
      props: { inst: baseInst, onClick: () => {} },
    });
    expect(queryByTestId('imported-card-modified-tag')).toBeNull();
  });
});

const inst: InstanceWithStatus = {
  id: 'a',
  name: 'My Pack',
  mc_version: '1.20.1',
  loader: 'fabric',
  loader_version: '0.16.5',
  max_heap_mb: 4096,
  extra_jvm_args: '',
  created_unix_ms: 1_700_000_000_000,
  ready: true,
  mrpack_name: 'All The Mods 10',
  mrpack_version: '1.4.7',
  mrpack_project_id: 'p',
  mrpack_source: 'modrinth',
  mrpack_summary: null,
  mrpack_version_id: 'v1',
  integrity: null,
};

afterEach(() => modpackUpdates.reset());

describe('ImportedCard update badge', () => {
  it('shows the update badge when the store reports update_available', async () => {
    vi.spyOn(commands, 'modpacksCheckUpdates').mockResolvedValue({
      status: 'ok',
      data: [
        {
          instance_id: 'a',
          status: {
            kind: 'update_available',
            entry: {
              id: 'v2',
              name: 'P',
              version_number: '1.5.0',
              game_versions: [],
              loaders: [],
              date_published: '',
            },
          },
        },
      ],
    });
    await modpackUpdates.sweep(['a'], { force: true });
    const { getByTestId } = render(ImportedCard, { props: { inst, onClick: () => {} } });
    expect(getByTestId('imported-card-update-tag').textContent).toContain('1.5.0');
  });

  it('renders no update badge when up to date', () => {
    const { queryByTestId } = render(ImportedCard, { props: { inst, onClick: () => {} } });
    expect(queryByTestId('imported-card-update-tag')).toBeNull();
  });
});
