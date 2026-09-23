// tests/appearance-panel.test.ts
import { render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

// The panel reads the account list to explain why "Add account" stays visible.
const { listAccounts } = vi.hoisted(() => ({ listAccounts: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listAccounts: () => listAccounts(),
    appSettingsPatchGeneral: vi.fn(async () => ({ status: 'ok', data: {} })),
    appSettingsGet: vi.fn(async () => ({ status: 'ok', data: { general: {} } })),
  },
}));

import { rainbowFx } from '../src/lib/fx/rainbow-fx.svelte';
import { langPref } from '../src/lib/i18n/state.svelte';
import { SIDEBAR_BUTTONS } from '../src/lib/layout/sidebar-buttons';
import AppearancePanel from '../src/lib/settings/AppearancePanel.svelte';
import { themeState } from '../src/lib/theme/state.svelte';

const AN_ACCOUNT = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

beforeEach(() => {
  vi.clearAllMocks();
  listAccounts.mockResolvedValue({ status: 'ok', data: [AN_ACCOUNT] });
  themeState.pref = 'system';
  themeState.systemDark = false;
});

describe('AppearancePanel', () => {
  it('renders the three theme options as aria-pressed buttons inside one group named "Theme"', () => {
    render(AppearancePanel);
    const groups = screen.getAllByRole('group', { name: 'Theme' });
    expect(groups).toHaveLength(1);
    for (const v of ['system', 'light', 'dark']) {
      const btn = screen.getByTestId(`theme-${v}`);
      expect(btn.tagName).toBe('BUTTON');
      expect(btn.getAttribute('aria-pressed')).not.toBeNull();
      expect(groups[0].contains(btn)).toBe(true);
    }
  });

  it('describes the theme group with the hint under it', () => {
    render(AppearancePanel);
    const group = screen.getByRole('group', { name: 'Theme' });
    const id = group.getAttribute('aria-describedby');
    expect(id).toBeTruthy();
    expect(document.getElementById(id as string)?.textContent).toContain('operating system');
  });

  it('opens the theme block with a heading, so the search label word is on the page', () => {
    render(AppearancePanel);
    expect(screen.getByRole('heading', { name: 'Theme', level: 3 })).toBeTruthy();
  });

  it('groups the two icon effects under an "Effects" heading', () => {
    render(AppearancePanel);
    expect(screen.getByRole('heading', { name: 'Effects', level: 3 })).toBeTruthy();
  });

  it('renders the language selector', () => {
    render(AppearancePanel);
    expect(screen.getByTestId('language-select')).toBeTruthy();
  });

  it('rainbow toggle reflects the rainbow preference', () => {
    rainbowFx.set(true);
    render(AppearancePanel);
    const toggle = screen.getByTestId('rainbow-icons-toggle') as HTMLInputElement;
    expect(toggle.checked).toBe(true);
  });

  it('the sidebar checklist is a group named Sidebar buttons whose legend holds the block heading', () => {
    render(AppearancePanel);
    // A fieldset names every checkbox inside it ("Mods, checkbox, checked — Sidebar
    // buttons"); the legend wraps the h3 so the block stays in heading navigation.
    const group = screen.getByRole('group', { name: 'Sidebar buttons' });
    expect(group.tagName).toBe('FIELDSET');
    expect(group.querySelector('legend h3')?.textContent?.trim()).toBe('Sidebar buttons');
    expect(group.querySelectorAll('input[type="checkbox"]').length).toBe(SIDEBAR_BUTTONS.length);
  });

  it('on System, the theme hint says which theme that currently resolves to', async () => {
    themeState.pref = 'system';
    themeState.systemDark = true;
    render(AppearancePanel);
    const hint = document.getElementById('appearance-theme-hint') as HTMLElement;
    await waitFor(() => expect(hint.textContent).toMatch(/currently dark/i));
  });

  it('says nothing about the resolved theme when the OS preference could not be read', async () => {
    // "Could not tell" is not "light". readSystemPrefersDark answers null when
    // matchMedia is unavailable, and the label must stay silent rather than
    // print the guess the painter has to make.
    themeState.pref = 'system';
    themeState.systemDark = null;
    render(AppearancePanel);
    const hint = document.getElementById('appearance-theme-hint') as HTMLElement;
    await waitFor(() => expect(hint).toBeTruthy());
    // Only the appended suffix is forbidden — the hint's own sentence has
    // always mentioned "light or dark setting", and that stays.
    expect(hint.textContent).not.toMatch(/currently/i);
  });

  it('explains why Add account stays visible while there is no account', async () => {
    listAccounts.mockResolvedValue({ status: 'ok', data: [] });
    render(AppearancePanel);
    await waitFor(() => expect(screen.getByText(/this button can't be hidden yet/i)).toBeTruthy());
  });

  it('drops the explanation once an account exists', async () => {
    listAccounts.mockResolvedValue({ status: 'ok', data: [AN_ACCOUNT] });
    render(AppearancePanel);
    await waitFor(() => expect(screen.getByTestId('sidebar-button-toggle-skin')).toBeTruthy());
    expect(screen.queryByText(/this button can't be hidden yet/i)).toBeNull();
  });

  it('claims nothing about the override when the account list could not be read', async () => {
    listAccounts.mockResolvedValue({ status: 'error', error: { kind: 'io', details: 'nope' } });
    render(AppearancePanel);
    await waitFor(() => expect(screen.getByTestId('sidebar-button-toggle-skin')).toBeTruthy());
    expect(screen.queryByText(/this button can't be hidden yet/i)).toBeNull();
  });

  it("names the buttons that are a feature's only way in", async () => {
    render(AppearancePanel);
    const note = await screen.findByTestId('sidebar-one-way-note');
    expect(note.textContent).toContain('Servers');
    expect(note.textContent).toContain('Skin and cape');
    expect(note.textContent).toContain('Import from launcher');
    // Gallery keeps a per-instance surface, so it is named with its qualifier
    // rather than listed as one-way or left out entirely.
    expect(note.textContent).toContain('Gallery');
  });

  it('on System, the language control says which language that currently resolves to', () => {
    const real = navigator.language;
    Object.defineProperty(navigator, 'language', { value: 'ru-RU', configurable: true });
    try {
      langPref.value = 'system';
      render(AppearancePanel);
      // The autonym, as the other options name themselves.
      expect(screen.getByTestId('language-select').textContent).toContain('Русский');
    } finally {
      Object.defineProperty(navigator, 'language', { value: real, configurable: true });
    }
  });

  it('says only "System" for the language when the OS language could not be read', () => {
    const real = navigator.language;
    Object.defineProperty(navigator, 'language', { value: '', configurable: true });
    try {
      langPref.value = 'system';
      render(AppearancePanel);
      expect(screen.getByTestId('language-select').textContent).not.toMatch(/currently/i);
    } finally {
      Object.defineProperty(navigator, 'language', { value: real, configurable: true });
    }
  });
});
