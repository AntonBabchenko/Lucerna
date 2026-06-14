import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { ModpackHit } from '$lib/ipc/bindings';
import ModpackCard from '$lib/modpacks/ModpackCard.svelte';

const hit: ModpackHit = {
  project_id: 'layout-test',
  slug: 'layout-test',
  title: 'Layout Test Pack',
  description: 'Testing grid vs list layout',
  icon_url: null,
  downloads: 9999,
  latest_mc_version: '1.20.4',
  supported_loaders: ['fabric'],
  source: 'modrinth',
  distribution_allowed: null,
  author: null,
};

describe('ModpackCard layout', () => {
  it('grid mode does NOT render card-list-row', () => {
    const { queryByTestId } = render(ModpackCard, {
      props: { hit, onClick: () => {}, layout: 'grid' },
    });
    expect(queryByTestId('card-list-row')).toBeNull();
  });

  it('list mode renders card-list-row', () => {
    const { getByTestId } = render(ModpackCard, {
      props: { hit, onClick: () => {}, layout: 'list' },
    });
    expect(getByTestId('card-list-row')).toBeTruthy();
  });

  it('list mode shows the pack title', () => {
    const { getByText } = render(ModpackCard, {
      props: { hit, onClick: () => {}, layout: 'list' },
    });
    expect(getByText('Layout Test Pack')).toBeTruthy();
  });

  it('grid mode still renders the modpack-card testid', () => {
    const { getByTestId } = render(ModpackCard, {
      props: { hit, onClick: () => {}, layout: 'grid' },
    });
    expect(getByTestId('modpack-card')).toBeTruthy();
  });
});
