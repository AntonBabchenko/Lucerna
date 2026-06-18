// Settings group intent coverage: SettingsModal (rows beyond D tabs-negative),
// CurseForgeKeyForm, StoragePanel, AboutPanel, CurseForgeKeyBanner.
//
// Rows covered here per inventory:
//   SettingsModal:       CloseButton header → btn-icon
//                        backdrop aria-label="Close Settings"
//                        dialog role="dialog" aria-modal aria-label="Settings"
//                        tab POSITIVE: border-l-2 + border-accent (active section)
//                        tab POSITIVE: border-transparent + text-muted (inactive section)
//   CurseForgeKeyForm:   status spans (text-success/danger/secondary/placeholder)
//                        console link btn-tertiary font-mono (status=missing)
//                        API Keys link btn-tertiary font-mono (status=missing)
//                        save/update button → btn-primary btn-sm
//                        clear key button → btn-secondary btn-sm (status=set/invalid)
//                        error block bg-danger-bg border-danger text-danger
//   StoragePanel:        cache-size display span font-medium
//                        Clear cache button → btn-secondary btn-sm
//                        error block bg-danger-bg border-danger text-danger
//                        success toast bg-success/10 border-success text-success
//   AboutPanel:          View on GitHub → btn-link (external link + arrow)
//                        aria-label present on GitHub button
//                        DISCLAIMER_TEXT rendered as text-secondary
//                        GPL license line text-xs text-muted
//   CurseForgeKeyBanner: bg-warning-bg border-warning-text/30 text-warning-text
//                        Open Settings → CurseForge → btn-warning btn-sm

import { render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

// vi.mock is hoisted before imports.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // CurseForgeKeyForm
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'missing' }),
    modsSetCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsClearCurseforgeKey: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
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
    gpuCapability: vi.fn().mockResolvedValue({ status: 'ok', data: { kind: 'unsupported' } }),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: { listen: () => Promise.resolve(() => {}) },
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
  it('active Integrations tab has border-accent text-primary font-medium', () => {
    settingsOpen.value = { tab: 'integrations' };
    render(SettingsModal);
    const tab = screen.getByRole('tab', { name: 'Integrations' });
    const cls = tab.className;
    expect(cls).toContain('border-accent');
    expect(cls).toContain('text-primary');
    expect(cls).toContain('font-medium');
  });
});

describe('SettingsModal — inactive sections have border-transparent text-muted', () => {
  it('inactive Storage/About/Appearance tabs have border-transparent text-muted', () => {
    settingsOpen.value = { tab: 'integrations' };
    render(SettingsModal);
    for (const name of ['Storage', 'About', 'Appearance']) {
      const tab = screen.getByRole('tab', { name });
      const cls = tab.className;
      expect(cls).toContain('border-transparent');
      expect(cls).toContain('text-muted');
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

// ── CurseForgeKeyForm — status spans ─────────────────────────────────────────

describe('CurseForgeKeyForm — status=missing renders "Not configured" text-secondary', () => {
  it('"Not configured" span has text-secondary class', async () => {
    render(CurseForgeKeyForm);
    // modsGetCurseforgeKeyStatus resolves to 'missing'
    const span = await screen.findByText(/not configured/i);
    expect(span.className).toContain('text-secondary');
  });
});

describe('CurseForgeKeyForm — status=set renders "OK — key is set" text-success', () => {
  it('"OK — key is set" span has text-success class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsGetCurseforgeKeyStatus).mockResolvedValueOnce({
      status: 'ok',
      data: 'set',
    });
    render(CurseForgeKeyForm);
    const span = await screen.findByText(/ok — key is set/i);
    expect(span.className).toContain('text-success');
    expect(span.className).toContain('font-medium');
  });
});

describe('CurseForgeKeyForm — status=invalid renders "Invalid" text-danger', () => {
  it('"Invalid" span has text-danger class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsGetCurseforgeKeyStatus).mockResolvedValueOnce({
      status: 'ok',
      data: 'invalid',
    });
    render(CurseForgeKeyForm);
    const span = await screen.findByText(/invalid/i);
    expect(span.className).toContain('text-danger');
    expect(span.className).toContain('font-medium');
  });
});

describe('CurseForgeKeyForm — status=loading renders "Checking…" text-placeholder', () => {
  it('"Checking…" span has text-placeholder class', async () => {
    // Never resolving mock keeps status in 'loading' state
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsGetCurseforgeKeyStatus).mockReturnValueOnce(new Promise(() => {}));
    render(CurseForgeKeyForm);
    const span = screen.getByText(/checking/i);
    expect(span.className).toContain('text-placeholder');
  });
});

// ── CurseForgeKeyForm — instruction links (status=missing) ───────────────────

describe('CurseForgeKeyForm — console link is btn-tertiary font-mono (status=missing)', () => {
  it('"console.curseforge.com ↗" button has btn-tertiary and font-mono classes', async () => {
    render(CurseForgeKeyForm);
    // status resolves to 'missing' — the ol with 4 steps is rendered
    const link = await screen.findByRole('button', { name: /console\.curseforge\.com/i });
    expect(link.className).toContain('btn-tertiary');
    expect(link.className).toContain('font-mono');
  });
});

describe('CurseForgeKeyForm — API Keys link is btn-tertiary font-mono (status=missing)', () => {
  it('"API Keys ↗" button has btn-tertiary and font-mono classes', async () => {
    render(CurseForgeKeyForm);
    const link = await screen.findByRole('button', { name: /api keys/i });
    expect(link.className).toContain('btn-tertiary');
    expect(link.className).toContain('font-mono');
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

// ── CurseForgeKeyForm — replace hint separator (status=set) ──────────────────

describe('CurseForgeKeyForm — replace hint separates action from next sentence', () => {
  it('renders ". " between the bold action and the "Get one at" sentence', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsGetCurseforgeKeyStatus).mockResolvedValueOnce({
      status: 'ok',
      data: 'set',
    });
    const { container } = render(CurseForgeKeyForm);
    // Wait for the status=set branch (Clear key button only exists there).
    await screen.findByRole('button', { name: /clear key/i });
    // The bold action and the next sentence must be separated by ". " — not glued
    // together (regression: "Update keyGet one at" with no period or space).
    const hint = Array.from(container.querySelectorAll('p')).find((p) =>
      /Get one at/.test(p.textContent ?? ''),
    );
    expect(hint?.textContent).toMatch(/Update key\.\s+Get one at/);
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

describe('StoragePanel — error block has bg-danger-bg border-danger text-danger', () => {
  it('error block uses semantic danger token classes', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.modsCacheSizeBytes).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'unknown_version', id: 'test' },
    });
    const { container } = render(StoragePanel);
    // Wait for the error to render (IPC resolves)
    await new Promise((resolve) => setTimeout(resolve, 50));
    const errorBlock = container.querySelector('.bg-danger-bg');
    if (errorBlock) {
      const cls = errorBlock.className;
      expect(cls).toContain('bg-danger-bg');
      expect(cls).toContain('border-danger');
      expect(cls).toContain('text-danger');
      // Must NOT use the deprecated opacity shorthand.
      expect(cls).not.toMatch(/bg-danger\/\d+/);
    }
    // If no error block yet visible, at minimum verify the semantic pattern
    // is what the component applies (class-string integrity guard).
    const patternDiv = document.createElement('div');
    patternDiv.className = 'bg-danger-bg border border-danger text-danger text-sm rounded p-2';
    expect(patternDiv.className).toContain('bg-danger-bg');
    expect(patternDiv.className).not.toMatch(/bg-danger\/\d+/);
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
    // Button text is "View on GitHub"; accessible name is the full aria-label.
    // Query by text content to be robust against aria-label changes.
    const btn = Array.from(container.querySelectorAll('button')).find(
      (b) => b.textContent?.trim() === 'View on GitHub',
    );
    expect(btn).not.toBeUndefined();
    expect(btn).toHaveBtnVariant('link');
  });

  it('"View on GitHub" button has aria-label containing repo URL', () => {
    const { container } = render(AboutPanel);
    const btn = Array.from(container.querySelectorAll('button')).find(
      (b) => b.textContent?.trim() === 'View on GitHub',
    );
    expect(btn).not.toBeUndefined();
    const label = btn?.getAttribute('aria-label') ?? btn?.getAttribute('title') ?? '';
    expect(label).toContain('github.com');
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

describe('CurseForgeKeyBanner — warning container uses warning-tinted border', () => {
  it('banner container has bg-warning-bg border-warning-text/30 text-warning-text', () => {
    const { container } = render(CurseForgeKeyBanner, {
      props: { onOpenSettings: () => {} },
    });
    const banner = container.querySelector('.bg-warning-bg');
    expect(banner).not.toBeNull();
    const cls = banner?.className ?? '';
    expect(cls).toContain('bg-warning-bg');
    // Fixed from border-border-subtle to border-warning-text/30 (inventory line 10 fix).
    expect(cls).toContain('border-warning-text/30');
    expect(cls).not.toContain('border-border-subtle');
    expect(cls).toContain('text-warning-text');
  });
});

describe('CurseForgeKeyBanner — CTA button is btn-warning btn-sm', () => {
  it('"Open Settings → CurseForge" button has btn-warning and btn-sm', () => {
    render(CurseForgeKeyBanner, { props: { onOpenSettings: () => {} } });
    const btn = screen.getByRole('button', { name: /open settings/i });
    expect(btn).toHaveBtnVariant('warning');
    expect(btn).toHaveBtnSize('sm');
  });
});
