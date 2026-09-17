import { render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { changelogTranslations } from '$lib/changelog/translation.svelte';
import type { Changelog } from '$lib/changelog/types';
import { locale } from '$lib/i18n';

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));

import ChangelogPanel from '$lib/changelog/ChangelogPanel.svelte';

// The panel follows the UI locale: under `ru` it overlays the Russian
// changelog on the English structure and says, per version, when it had to
// fall back to English. Under the source language it never says anything.
const EN: Changelog = [
  {
    version: '0.2.0',
    date: '2026-02-02',
    url: null,
    sections: [{ kind: 'added', heading: 'Added', items: ['New thing.', 'Second entry.'] }],
  },
  {
    version: '0.1.0',
    date: '2026-01-01',
    url: null,
    sections: [{ kind: 'added', heading: 'Added', items: ['First release.'] }],
  },
];

const RU_FULL: Changelog = [
  {
    version: '0.2.0',
    date: '2026-02-02',
    url: null,
    sections: [{ kind: 'other', heading: 'Добавлено', items: ['Новая штука.', 'Вторая запись.'] }],
  },
  {
    version: '0.1.0',
    date: '2026-01-01',
    url: null,
    sections: [{ kind: 'other', heading: 'Добавлено', items: ['Первый выпуск.'] }],
  },
];

function reset(): void {
  for (const k of Object.keys(changelogTranslations)) delete changelogTranslations[k];
}

describe('ChangelogPanel under a non-source locale', () => {
  beforeEach(() => {
    reset();
    locale.set('ru');
  });
  afterEach(() => {
    reset();
    locale.set('en');
  });

  it('renders the translated bullets and localized section label, with no fallback note', () => {
    changelogTranslations.ru = { status: 'ready', entries: RU_FULL };
    render(ChangelogPanel, { props: { entries: EN } });
    expect(screen.getByText('Новая штука.')).toBeTruthy();
    expect(screen.getByText('Первый выпуск.')).toBeTruthy();
    expect(screen.getAllByText('Добавлено').length).toBe(2);
    expect(screen.queryByText('New thing.')).toBeNull();
    expect(screen.queryByTestId('changelog-fallback-note')).toBeNull();
    expect(screen.queryByTestId('changelog-version-note')).toBeNull();
  });

  it('shows a version in English with a note when the translation lacks it', () => {
    changelogTranslations.ru = { status: 'ready', entries: [RU_FULL[0]] };
    render(ChangelogPanel, { props: { entries: EN } });
    expect(screen.getByText('Новая штука.')).toBeTruthy();
    expect(screen.getByText('First release.')).toBeTruthy();
    const notes = screen.getAllByTestId('changelog-version-note');
    expect(notes.length).toBe(1);
    expect(notes[0].textContent).toMatch(/на английском/);
    expect(notes[0].textContent).not.toMatch(/Часть/);
  });

  it('shows a partly translated version mixed, with a "partly" note', () => {
    changelogTranslations.ru = {
      status: 'ready',
      entries: [
        {
          ...RU_FULL[0],
          sections: [{ ...RU_FULL[0].sections[0], items: ['Новая штука.', 'Second entry.'] }],
        },
        RU_FULL[1],
      ],
    };
    render(ChangelogPanel, { props: { entries: EN } });
    expect(screen.getByText('Новая штука.')).toBeTruthy();
    expect(screen.getByText('Second entry.')).toBeTruthy();
    const notes = screen.getAllByTestId('changelog-version-note');
    expect(notes.length).toBe(1);
    expect(notes[0].textContent).toMatch(/Часть/);
  });

  it('shows one panel-level note and no per-version notes when the locale has no changelog', () => {
    changelogTranslations.ru = { status: 'missing' };
    render(ChangelogPanel, { props: { entries: EN } });
    expect(screen.getByText('New thing.')).toBeTruthy();
    const note = screen.getByTestId('changelog-fallback-note');
    expect(note.textContent).toMatch(/на английском/);
    expect(screen.queryByTestId('changelog-version-note')).toBeNull();
  });

  it('distinguishes a translation that failed to load from one that does not exist', () => {
    changelogTranslations.ru = { status: 'failed', error: new Error('boom') };
    render(ChangelogPanel, { props: { entries: EN } });
    expect(screen.getByText('New thing.')).toBeTruthy();
    const note = screen.getByTestId('changelog-fallback-note');
    expect(note.textContent).toMatch(/Не удалось загрузить/);
    expect(screen.queryByTestId('changelog-version-note')).toBeNull();
  });

  it('shows English without any note while the translation is still loading', () => {
    // Nothing in the cache for `ru` yet: the effect kicks off a load of the
    // real ru.md, but until it lands the panel must not claim anything.
    render(ChangelogPanel, { props: { entries: EN } });
    expect(screen.getByText('New thing.')).toBeTruthy();
    expect(screen.queryByTestId('changelog-fallback-note')).toBeNull();
    expect(screen.queryByTestId('changelog-version-note')).toBeNull();
  });
});

describe('ChangelogPanel under the source locale', () => {
  it('never shows a fallback note, whatever the translation cache holds', () => {
    reset();
    locale.set('en');
    changelogTranslations.en = { status: 'missing' };
    render(ChangelogPanel, { props: { entries: EN } });
    expect(screen.getByText('New thing.')).toBeTruthy();
    expect(screen.queryByTestId('changelog-fallback-note')).toBeNull();
    expect(screen.queryByTestId('changelog-version-note')).toBeNull();
    reset();
  });
});
