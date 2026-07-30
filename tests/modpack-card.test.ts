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
  author: 'Pack Author',
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

  it('renders the author when present (grid)', () => {
    const { getAllByText } = render(ModpackCard, { props: { hit, onClick: () => {} } });
    expect(getAllByText(/Pack Author/).length).toBeGreaterThan(0);
  });

  it('renders the author when present (list)', () => {
    const { getAllByText } = render(ModpackCard, {
      props: { hit, onClick: () => {}, layout: 'list' },
    });
    expect(getAllByText(/Pack Author/).length).toBeGreaterThan(0);
  });

  it('shows no author line when author is null', () => {
    const noAuthor: ModpackHit = { ...hit, author: null };
    const { queryByText } = render(ModpackCard, { props: { hit: noAuthor, onClick: () => {} } });
    expect(queryByText(/Pack Author/)).toBeNull();
  });

  it('renders quick-install button when onQuickInstall is provided', () => {
    const { getByLabelText } = render(ModpackCard, {
      props: { hit, onClick: () => {}, onQuickInstall: () => {} },
    });
    expect(getByLabelText('Install latest version as a new instance')).toBeTruthy();
  });

  it('has no quick-install button when onQuickInstall is omitted', () => {
    const { queryByLabelText } = render(ModpackCard, { props: { hit, onClick: () => {} } });
    expect(queryByLabelText('Install latest version as a new instance')).toBeNull();
  });

  it('quick-install click fires onQuickInstall and not the card onClick (grid)', async () => {
    const onClick = vi.fn();
    const onQuickInstall = vi.fn();
    const { getByLabelText } = render(ModpackCard, {
      props: { hit, onClick, onQuickInstall },
    });
    await fireEvent.click(getByLabelText('Install latest version as a new instance'));
    expect(onQuickInstall).toHaveBeenCalledTimes(1);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('quick-install click fires onQuickInstall and not the card onClick (list)', async () => {
    const onClick = vi.fn();
    const onQuickInstall = vi.fn();
    const { getByLabelText } = render(ModpackCard, {
      props: { hit, onClick, onQuickInstall, layout: 'list' },
    });
    await fireEvent.click(getByLabelText('Install latest version as a new instance'));
    expect(onQuickInstall).toHaveBeenCalledTimes(1);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('disables the quick-install button while installing', () => {
    const { getByLabelText } = render(ModpackCard, {
      props: { hit, onClick: () => {}, onQuickInstall: () => {}, installing: true },
    });
    expect(
      (getByLabelText('Install latest version as a new instance') as HTMLButtonElement).disabled,
    ).toBe(true);
  });
});
