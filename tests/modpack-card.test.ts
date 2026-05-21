import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ModpackHit } from '$lib/ipc/bindings';
import ModpackCard from '$lib/modpacks/ModpackCard.svelte';

// ModpackHit fixture: project_id + slug + display fields cover the
// fields the card renders (title, description, downloads, icon_url).
// downloads is `number | null` on the bindings type — pin a concrete
// number here, the null branch is covered by ModpackCard's `?? 0`
// fallback at the call site.
const hit: ModpackHit = {
  project_id: 'p',
  slug: 's',
  title: 'Cool Pack',
  description: 'desc',
  icon_url: 'https://cdn.modrinth.com/icon.png',
  downloads: 12345,
  latest_mc_version: '1.20.1',
  supported_loaders: ['fabric'],
  source: 'modrinth',
  distribution_allowed: null,
};

describe('ModpackCard', () => {
  it('renders title and description', () => {
    const { getByText } = render(ModpackCard, { props: { hit, onClick: () => {} } });
    expect(getByText('Cool Pack')).toBeTruthy();
    expect(getByText('desc')).toBeTruthy();
  });

  it('fires onClick when clicked', async () => {
    const onClick = vi.fn();
    const { getByTestId } = render(ModpackCard, { props: { hit, onClick } });
    await fireEvent.click(getByTestId('modpack-card'));
    expect(onClick).toHaveBeenCalled();
  });

  it('shows the distribution-disabled badge when distribution_allowed is false', () => {
    const blocked: ModpackHit = { ...hit, source: 'curseforge', distribution_allowed: false };
    const { getByText } = render(ModpackCard, { props: { hit: blocked, onClick: () => {} } });
    expect(getByText('CurseForge download disabled')).toBeTruthy();
  });

  it('shows no badge when distribution is allowed', () => {
    const { queryByText } = render(ModpackCard, { props: { hit, onClick: () => {} } });
    expect(queryByText('CurseForge download disabled')).toBeNull();
  });
});
