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
});
