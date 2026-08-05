import { fireEvent, render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
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

const QUICK_INSTALL_LABEL =
  "Install this pack's newest version as a new instance. Its Minecraft version and loader come from that pack version.";
const QUICK_INSTALL_FILTERED_LABEL =
  "Install this pack's newest version for Minecraft 1.20.1 as a new instance.";

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
    expect(getByLabelText(QUICK_INSTALL_LABEL)).toBeTruthy();
  });

  it('has no quick-install button when onQuickInstall is omitted', () => {
    const { queryByLabelText } = render(ModpackCard, { props: { hit, onClick: () => {} } });
    expect(queryByLabelText(QUICK_INSTALL_LABEL)).toBeNull();
  });

  it('quick-install click fires onQuickInstall and not the card onClick (grid)', async () => {
    const onClick = vi.fn();
    const onQuickInstall = vi.fn();
    const { getByLabelText } = render(ModpackCard, {
      props: { hit, onClick, onQuickInstall },
    });
    await fireEvent.click(getByLabelText(QUICK_INSTALL_LABEL));
    expect(onQuickInstall).toHaveBeenCalledTimes(1);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('quick-install click fires onQuickInstall and not the card onClick (list)', async () => {
    const onClick = vi.fn();
    const onQuickInstall = vi.fn();
    const { getByLabelText } = render(ModpackCard, {
      props: { hit, onClick, onQuickInstall, layout: 'list' },
    });
    await fireEvent.click(getByLabelText(QUICK_INSTALL_LABEL));
    expect(onQuickInstall).toHaveBeenCalledTimes(1);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('disables the quick-install button while installing', () => {
    const { getByLabelText } = render(ModpackCard, {
      props: { hit, onClick: () => {}, onQuickInstall: () => {}, installing: true },
    });
    expect((getByLabelText(QUICK_INSTALL_LABEL) as HTMLButtonElement).disabled).toBe(true);
  });

  it('explains the selection rule in the quick-install tooltip', () => {
    const { getByLabelText } = render(ModpackCard, {
      props: { hit, onClick: () => {}, onQuickInstall: () => {} },
    });
    expect(getByLabelText(QUICK_INSTALL_LABEL).getAttribute('aria-label')).toBe(
      QUICK_INSTALL_LABEL,
    );
  });

  // With the browse toolbar's MC filter set, the pick is the newest version
  // matching *that* filter — the tooltip has to say so, or "latest" is a lie.
  it('names the active MC filter in the quick-install tooltip', () => {
    const { getByLabelText } = render(ModpackCard, {
      props: { hit, onClick: () => {}, onQuickInstall: () => {}, mcFilter: '1.20.1' },
    });
    expect(getByLabelText(QUICK_INSTALL_FILTERED_LABEL)).toBeTruthy();
  });
});

// The download counter goes through an ICU plural message in ru. Passing a
// pre-formatted string as the plural argument makes intl-messageformat coerce
// it with Number(), which yields NaN — rendered by Intl.NumberFormat('ru') as
// the literal "не число". Assert the number survives, in both layouts.
//
// The expected separator comes from Intl, not a hardcoded literal: ru groups
// with a non-breaking space whose exact codepoint is an ICU-data detail.
describe('ModpackCard download count', () => {
  const ru = new Intl.NumberFormat('ru').format(12345);
  const en = new Intl.NumberFormat('en').format(12345);

  // Every other case in this file asserts English copy, so the locale must be
  // put back — a leaked 'ru' would fail them in file order.
  afterEach(() => locale.set('en'));

  it('renders a formatted number in English (grid)', () => {
    locale.set('en');
    const { container } = render(ModpackCard, { props: { hit, onClick: () => {} } });
    expect(container.textContent).toContain(en);
  });

  describe('in Russian', () => {
    beforeEach(() => locale.set('ru'));

    it('renders a formatted number, not NaN (grid)', () => {
      const { container } = render(ModpackCard, { props: { hit, onClick: () => {} } });
      expect(container.textContent).not.toContain('не число');
      expect(container.textContent).toContain(ru);
    });

    it('renders a formatted number, not NaN (list)', () => {
      const { container } = render(ModpackCard, {
        props: { hit, onClick: () => {}, layout: 'list' },
      });
      expect(container.textContent).not.toContain('не число');
      expect(container.textContent).toContain(ru);
    });

    it('picks the plural category from the count', () => {
      const { container } = render(ModpackCard, {
        props: { hit: { ...hit, downloads: 1 }, onClick: () => {} },
      });
      expect(container.textContent).toContain('1 скачивание');
    });
  });
});
