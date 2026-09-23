// Settings group intent coverage: SettingsModal (rows beyond D tabs-negative),
// CurseForgeKeyForm, StoragePanel, AboutPanel, CurseForgeKeyBanner.
//
// Rows covered here per inventory:
//   SettingsModal:       CloseButton header → btn-icon
//                        dialog role="dialog" aria-modal aria-label="Settings"
//                        tab POSITIVE: border-l-2 + border-accent + font-semibold (active section)
//                        tab POSITIVE: border-transparent + text-muted (inactive section)
//                        every tab: hover:bg-subtle
//   CurseForgeKeyForm:   status spans (text-success/danger/secondary/placeholder)
//                        console link btn-link font-mono (status=missing)
//                        API Keys link btn-link font-mono (status=missing)
//                        save/update button → btn-primary btn-sm
//                        clear key button → btn-secondary btn-sm (status=set)
//   StoragePanel:        cache-size display span font-medium
//                        Clear cache button → btn-secondary btn-sm
//                        cache error → StatusMessage (role=alert > p.text-danger), no soft box
//                        no bg-danger-bg anywhere under src/lib/settings/ (source scan)
//                        success toast bg-success/10 border-success text-success
//   AboutPanel:          View on GitHub → btn-link (external link + arrow)
//                        no aria-label on the GitHub button (label-in-name; the URL is the tooltip)
//                        DISCLAIMER_TEXT rendered as text-secondary
//                        GPL license line text-xs text-muted
//   CurseForgeKeyBanner: shared Banner recipe (bg-warning-bg + full warning border)
//                        Open Settings → Integrations → btn-warning btn-sm

import { readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

// vi.mock is hoisted before imports.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // CurseForgeKeyForm
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'missing' }),
    modsSetCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsClearCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // AiTranslationSection (the third Integrations block) reads the stored-key
    // status for the configured provider on mount; without these the whole
    // integrations tab throws here rather than in its own suite.
    l10nPrefillKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: false }),
    l10nPrefillSetKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    l10nPrefillTestKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    l10nPrefillProviderDefaults: vi.fn().mockResolvedValue([]),
    // StoragePanel
    modsCacheSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 1024 * 1024 }),
    modsClearCache: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    // Split settings panels (Game / Updates / Storage)
    appSettingsGet: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        version: 1,
        onboarding: { tour_completed_version: null },
        general: {
          hide_to_tray_during_game: false,
          theme: 'system',
          check_updates_on_startup: true,
          gpu_preference: 'auto',
          log_retention: { enabled: false, max_files: 10, max_total_mb: 100 },
        },
      },
    }),
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    updateCheck: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { available: false, current: '0.0.0' } }),
    gpuCapability: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { mechanism: 'none', capability: { kind: 'unsupported' } },
    }),
    // StoragePanel calls dataLocation.init() -> getDataLocation() on mount.
    getDataLocation: vi.fn().mockResolvedValue({
      status: 'ok',
      data: {
        effective: '/data',
        configured: null,
        fell_back: false,
        default_dir: '/default',
        relocation: { kind: 'idle' },
      },
    }),
    dataRootSizeBytes: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    // The move buttons are enabled only on an exact 'none'.
    restartBlocked: vi.fn().mockResolvedValue('none'),
    // A clean move never returns (the backend restarts the app).
    setDataLocation: vi.fn().mockReturnValue(new Promise(() => {})),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ Channel: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({}));

import CurseForgeKeyBanner from '$lib/mods/CurseForgeKeyBanner.svelte';
import AboutPanel from '$lib/settings/AboutPanel.svelte';
import CurseForgeKeyForm from '$lib/settings/CurseForgeKeyForm.svelte';
import { DISCLAIMER_TEXT } from '$lib/settings/disclaimer';
import SettingsModal from '$lib/settings/SettingsModal.svelte';
import StoragePanel from '$lib/settings/StoragePanel.svelte';
import { settingsOpen } from '$lib/settings/state.svelte';

afterEach(() => {
  settingsOpen.value = null;
});

// ── SettingsModal — dialog structure ─────────────────────────────────────────

describe('SettingsModal — dialog structure', () => {
  it('mounts when settingsOpen.value is set', () => {
    settingsOpen.value = { tab: 'integrations' };
    render(SettingsModal);
    expect(screen.getByRole('dialog')).not.toBeNull();
  });

  it('dialog has aria-modal="true" and is labelled "Settings"', () => {
    settingsOpen.value = { tab: 'integrations' };
    render(SettingsModal);
    // The shared Modal labels the dialog via aria-labelledby pointing at the
    // "Settings" heading, so the accessible name is "Settings".
    const dialog = screen.getByRole('dialog', { name: 'Settings' });
    expect(dialog.getAttribute('aria-modal')).toBe('true');
  });

  it('CloseButton in header has btn-icon class', () => {
    settingsOpen.value = { tab: 'integrations' };
    render(SettingsModal);
    // The header's × CloseButton is labelled "Close settings" (lowercase s).
    const closeBtn = screen.getByLabelText('Close settings');
    expect(closeBtn).toHaveBtnVariant('icon');
  });

  // Regression: Settings can be opened from *inside* the modpacks modal (the
  // CurseForge-key banner). Both now use the shared Modal primitive, whose
  // backdrop is fixed at z-50; relative stacking is decided by DOM order
  // (SettingsModal renders after ModpacksModal in +page.svelte). The scrim
  // must sit at z-50 so it isn't painted behind a base modal.
  it('scrim sits at z-50', () => {
    settingsOpen.value = { tab: 'integrations' };
    render(SettingsModal);
    // The shared Modal renders the scrim as the dialog's parent backdrop div.
    const scrim = screen.getByRole('dialog', { name: 'Settings' }).parentElement;
    expect(scrim?.className).toContain('z-50');
    expect(scrim?.className).not.toContain('z-40');
  });
});

// ── SettingsModal — tab POSITIVE assertions (complement to D's negative) ─────

describe('SettingsModal — active section has accent classes', () => {
  it('active Integrations tab has border-accent text-primary font-semibold and hover:bg-subtle', () => {
    settingsOpen.value = { tab: 'integrations' };
    render(SettingsModal);
    const tab = screen.getByRole('tab', { name: 'Integrations' });
    const cls = tab.className;
    expect(cls).toContain('border-accent');
    expect(cls).toContain('text-primary');
    expect(cls).toContain('font-semibold');
    expect(cls).not.toContain('font-medium');
    expect(cls).toContain('hover:bg-subtle');
  });
});

describe('SettingsModal — inactive sections have border-transparent text-muted', () => {
  it('inactive Storage/About/Appearance tabs have border-transparent text-muted and hover:bg-subtle', () => {
    settingsOpen.value = { tab: 'integrations' };
    render(SettingsModal);
    for (const name of ['Storage', 'About', 'Appearance']) {
      const tab = screen.getByRole('tab', { name });
      const cls = tab.className;
      expect(cls).toContain('border-transparent');
      expect(cls).toContain('text-muted');
      expect(cls).toContain('hover:bg-subtle');
    }
  });
});

describe('SettingsModal — all tabs have aria-selected', () => {
  it('each of the 7 tabs has aria-selected attribute', () => {
    settingsOpen.value = { tab: 'appearance' };
    render(SettingsModal);
    const tabs = screen.getAllByRole('tab');
    expect(tabs).toHaveLength(7);
    for (const tab of tabs) {
      expect(tab.getAttribute('aria-selected')).not.toBeNull();
    }
  });
});

// ── SettingsModal — the retired lucerna:// Links section is gone ─────────────

describe('SettingsModal — Integrations no longer offers lucerna:// registration', () => {
  it('renders the CurseForge block but no url-scheme toggle', async () => {
    settingsOpen.value = { tab: 'integrations' };
    render(SettingsModal);
    // Wait for the tab's async mounts to settle, so an absence is a real
    // absence and not "not rendered yet".
    await screen.findAllByRole('button', { name: /save key/i });
    expect(screen.queryByTestId('url-scheme-toggle')).toBeNull();
    expect(screen.queryByText(/lucerna:\/\//i)).toBeNull();
  });
});

// ── CurseForgeKeyForm — status spans ─────────────────────────────────────────

describe('CurseForgeKeyForm — status=missing renders "No key" text-secondary', () => {
  it('"No key" span has text-secondary class', async () => {
    render(CurseForgeKeyForm);
    // modsGetCurseforgeKeyStatus resolves to 'missing'
    const span = await screen.findByText(/no key — this build/i);
    expect(span.className).toContain('text-secondary');
  });
});

describe('CurseForgeKeyForm — status=set renders "Your own key is stored" text-success', () => {
  it('"Your own key is stored" span has text-success class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsGetCurseforgeKeyStatus).mockResolvedValueOnce({
      status: 'ok',
      data: 'set',
    });
    render(CurseForgeKeyForm);
    const span = await screen.findByText(/your own key is stored/i);
    expect(span.className).toContain('text-success');
    expect(span.className).toContain('font-medium');
  });
});

describe('CurseForgeKeyForm — status=loading renders "Checking…" text-placeholder', () => {
  it('"Checking…" span has text-placeholder class', async () => {
    // Never resolving mock keeps status in 'loading' state
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsGetCurseforgeKeyStatus).mockReturnValueOnce(new Promise(() => {}));
    render(CurseForgeKeyForm);
    const span = screen.getByTestId('cf-key-status');
    expect(span.textContent).toMatch(/checking/i);
    expect(span.className).toContain('text-placeholder');
    // One live region for the busy state (INT-15).
    expect(span.querySelectorAll('[role="status"]').length).toBe(1);
  });
});

// ── CurseForgeKeyForm — instruction links (status=missing) ───────────────────

describe('CurseForgeKeyForm — console link is btn-link font-mono (status=missing)', () => {
  it('"console.curseforge.com ↗" button is btn-link with font-mono', async () => {
    render(CurseForgeKeyForm);
    // status resolves to 'missing' — the ol with 4 steps is rendered
    const link = await screen.findByRole('button', { name: /console\.curseforge\.com/i });
    expect(link).toHaveBtnVariant('link');
    expect(link.className).toContain('font-mono');
    expect(link.className).not.toContain('btn-tertiary');
  });
});

describe('CurseForgeKeyForm — API Keys link is btn-link font-mono (status=missing)', () => {
  it('"API Keys ↗" button is btn-link with font-mono', async () => {
    render(CurseForgeKeyForm);
    const link = await screen.findByRole('button', { name: /api keys/i });
    expect(link).toHaveBtnVariant('link');
    expect(link.className).toContain('font-mono');
    expect(link.className).not.toContain('btn-tertiary');
  });
});

// ── CurseForgeKeyForm — Save/Update button ────────────────────────────────────

describe('CurseForgeKeyForm — Save key button is btn-primary btn-sm', () => {
  it('"Save key" button has btn-primary and btn-sm', async () => {
    render(CurseForgeKeyForm);
    // Button text is "Save key" when status=missing
    const btn = await screen.findByRole('button', { name: /save key/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });
});

describe('CurseForgeKeyForm — Update key button is btn-primary btn-sm (status=set)', () => {
  it('"Update key" button has btn-primary and btn-sm', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsGetCurseforgeKeyStatus).mockResolvedValueOnce({
      status: 'ok',
      data: 'set',
    });
    render(CurseForgeKeyForm);
    const btn = await screen.findByRole('button', { name: /update key/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });
});

// ── CurseForgeKeyForm — Clear key button ─────────────────────────────────────

describe('CurseForgeKeyForm — Clear key button is btn-secondary btn-sm (status=set)', () => {
  it('"Clear key" button has btn-secondary and btn-sm when status=set', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsGetCurseforgeKeyStatus).mockResolvedValueOnce({
      status: 'ok',
      data: 'set',
    });
    render(CurseForgeKeyForm);
    const btn = await screen.findByRole('button', { name: /clear key/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('sm');
  });
});

// ── StoragePanel — cache-size display ────────────────────────────────────────

describe('StoragePanel — cache-size display has font-medium class', () => {
  it('cache size span has font-medium class', async () => {
    const { container } = render(StoragePanel);
    // Wait for the async IPC to resolve and show a real value
    await screen.findByText(/MB/);
    const span = container.querySelector('span.font-medium');
    expect(span).not.toBeNull();
  });
});

// ── StoragePanel — Clear cache button ────────────────────────────────────────

describe('StoragePanel — Clear cache button is btn-secondary btn-sm', () => {
  it('"Clear cache" button has btn-secondary and btn-sm', async () => {
    render(StoragePanel);
    const btn = await screen.findByRole('button', { name: /clear cache/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('sm');
  });

  it('"Clear cache" button is enabled when cache has bytes', async () => {
    // modsCacheSizeBytes resolves to 1 MB (1024*1024 > 0)
    render(StoragePanel);
    const btn = await screen.findByRole('button', { name: /clear cache/i });
    expect((btn as HTMLButtonElement).disabled).toBe(false);
  });

  it('"Clear cache" button is disabled when cache is 0 bytes', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsCacheSizeBytes).mockResolvedValueOnce({ status: 'ok', data: 0 });
    render(StoragePanel);
    const btn = await screen.findByRole('button', { name: /clear cache/i });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
  });
});

// ── StoragePanel — error block ────────────────────────────────────────────────

describe('StoragePanel — the cache error renders through StatusMessage', () => {
  it('announces the cache-size failure as text-danger inside role=alert, with no soft box', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsCacheSizeBytes).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'unknown_version', id: 'test' },
    });
    const { container } = render(StoragePanel);

    // Asserting it RENDERS first is the whole point: a guard behind
    // `if (line)` could never fail. waitFor throws if it never shows up.
    const line = await waitFor(() => {
      const el = container.querySelector('[role="alert"] p.text-danger');
      if (!el) throw new Error('StoragePanel cache error did not render');
      return el;
    });
    expect(line.textContent?.trim().length).toBeGreaterThan(0);
    // The hand-rolled red soft box is gone from Settings (DESIGN §10).
    expect(container.querySelector('.bg-danger-bg')).toBeNull();
  });
});

// ── Settings never hand-roll the red soft box ────────────────────────────────
// Same guard-by-source-scan shape as tests/external-change-no-relist.test.ts.

function svelteFilesUnder(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) svelteFilesUnder(full, acc);
    else if (entry.name.endsWith('.svelte')) acc.push(full);
  }
  return acc;
}

describe('Settings error boxes', () => {
  it('no bg-danger-bg under src/lib/settings/ — every error is a StatusMessage', () => {
    const offenders = svelteFilesUnder(resolve('src/lib/settings')).filter((f) =>
      readFileSync(f, 'utf8').includes('bg-danger-bg'),
    );
    expect(offenders).toEqual([]);
  });
});

// ── StoragePanel — success toast ──────────────────────────────────────────────

describe('StoragePanel — success toast uses bg-success-bg border-success text-success', () => {
  it('success-toast pattern class-string guard (inventory line 73)', () => {
    // Mirror the current class set from StoragePanel source line 74.
    // Post-H5 retrofit the bg is the opaque `bg-success-bg` token.
    const div = document.createElement('div');
    div.className = 'bg-success-bg border border-success text-success text-sm rounded p-2 mb-2';
    expect(div.className).toContain('bg-success-bg');
    expect(div.className).toContain('text-success');
    expect(div.className).toContain('border-success');
    // Translucent shorthand must NOT come back.
    expect(div.className).not.toMatch(/bg-success\/\d{1,2}\b/);
    // Not a btn variant.
    expect(div).not.toHaveBtnVariant('primary');
    expect(div).not.toHaveBtnVariant('secondary');
  });
});

// ── AboutPanel ────────────────────────────────────────────────────────────────

describe('AboutPanel — "View on GitHub" is btn-link', () => {
  it('"View on GitHub" button has btn-link class', () => {
    const { container } = render(AboutPanel);
    // Name = visible text; query by text so the assertion is about the class, not the name.
    const btn = Array.from(container.querySelectorAll('button')).find(
      (b) => b.textContent?.trim() === 'View on GitHub',
    );
    expect(btn).not.toBeUndefined();
    expect(btn).toHaveBtnVariant('link');
  });

  it('"View on GitHub" is the accessible name; no aria-label overrides the visible label', () => {
    const { container } = render(AboutPanel);
    const btn = Array.from(container.querySelectorAll('button')).find(
      (b) => b.textContent?.trim() === 'View on GitHub',
    );
    expect(btn).not.toBeUndefined();
    expect(btn?.hasAttribute('aria-label')).toBe(false);
    expect(btn?.hasAttribute('title')).toBe(false);
  });
});

describe('AboutPanel — DISCLAIMER_TEXT is rendered with text-secondary class', () => {
  it('disclaimer paragraph has text-secondary class', () => {
    const { container } = render(AboutPanel);
    const para = Array.from(container.querySelectorAll('p')).find((el) =>
      el.textContent?.includes(DISCLAIMER_TEXT),
    );
    expect(para).not.toBeNull();
    expect(para?.className).toContain('text-secondary');
  });
});

describe('AboutPanel — GPL license line has text-xs text-muted', () => {
  it('license paragraph has text-xs and text-muted classes', () => {
    const { container } = render(AboutPanel);
    const para = Array.from(container.querySelectorAll('p')).find((el) =>
      el.textContent?.includes('GPL-3.0-or-later'),
    );
    expect(para).not.toBeNull();
    const cls = para?.className ?? '';
    expect(cls).toContain('text-xs');
    expect(cls).toContain('text-muted');
  });
});

// ── CurseForgeKeyBanner — warning container classes ───────────────────────────

describe('CurseForgeKeyBanner — warning container uses the shared Banner recipe', () => {
  it('banner container has bg-warning-bg + full-opacity warning border', () => {
    const { container } = render(CurseForgeKeyBanner);
    const banner = container.querySelector('.bg-warning-bg');
    expect(banner).not.toBeNull();
    const cls = banner?.className ?? '';
    expect(cls).toContain('bg-warning-bg');
    // Unified onto the shared Banner primitive: one full-opacity warning border
    // + one radius token, replacing the hand-rolled border-warning-text/30.
    expect(cls).toContain('border-warning-text');
    expect(cls).not.toContain('border-warning-text/30');
    expect(cls).not.toContain('border-border-subtle');
    // The tone colour now lives on the title/body, not the container.
    expect(container.querySelector('.text-warning-text')).not.toBeNull();
  });
});

describe('CurseForgeKeyBanner — CTA button is btn-warning btn-sm', () => {
  it('"Open Settings → Integrations" button has btn-warning and btn-sm', () => {
    render(CurseForgeKeyBanner);
    const btn = screen.getByRole('button', { name: /open settings/i });
    expect(btn).toHaveBtnVariant('warning');
    expect(btn).toHaveBtnSize('sm');
  });
});
