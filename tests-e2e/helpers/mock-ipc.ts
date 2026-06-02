// Playwright helper: inject a fake window.__TAURI_INTERNALS__ before the
// page loads any scripts so that the SvelteKit app can mount in plain
// Chromium (no Rust backend).
//
// How Tauri 2 IPC works in the browser:
//   @tauri-apps/api/core's `invoke()` calls
//   `window.__TAURI_INTERNALS__.invoke(cmd, args, options)`.
//   tauri-specta-generated commands pass the raw snake_case command name
//   (e.g. "list_accounts") — no "plugin:tauri-specta|" prefix.
//
//   @tauri-apps/api/event's `listen()` calls
//   `window.__TAURI_INTERNALS__.invoke('plugin:event|listen', ...)` and
//   stores a numeric handler id returned by `transformCallback`. For
//   tests we never fire real events, so listen() just returns an unlisten
//   no-op.
//
// All handlers return valid empty/default shapes so the UI mounts without
// throwing. Unknown commands return null (safe default — the UI guards
// on `.status === 'ok'` before reading `.data`).

import type { Page } from '@playwright/test';

// Minimal inline shapes mirroring src/lib/ipc/bindings.ts.
// We re-declare them here because tests-e2e/ is outside the SvelteKit
// tsconfig include glob; importing from $lib would require path-alias
// plumbing that Playwright's tsconfig does not have.

export type MockAccount = {
  id: string;
  name: string;
  uuid: string;
  expires_at: number | null;
};

export type MockInstance = {
  id: string;
  name: string;
  mc_version: string;
  loader: 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge';
  loader_version: string | null;
  max_heap_mb: number;
  extra_jvm_args: string;
  created_unix_ms: number | null;
  ready: boolean;
  mrpack_name: string | null;
  mrpack_version: string | null;
  mrpack_project_id: string | null;
  mrpack_source: 'modrinth' | 'curseforge' | null;
  mrpack_summary: string | null;
  mrpack_version_id: string | null;
};

export type MockState = {
  /** Stored accounts; defaults to empty. */
  accounts?: MockAccount[];
  /** Id of the currently-active account; defaults to null. */
  active_account_id?: string | null;
  /** Stored instances; defaults to empty. */
  instances?: MockInstance[];
  /** Id of the currently-active instance; defaults to null. */
  active_instance_id?: string | null;
  /** Simulated running process; defaults to null. */
  running?: { pid: number; version_id: string } | null;
  /** Whether an install is in progress; defaults to false. */
  installing?: boolean;
  /** Override for app_settings_get.general.theme; defaults to 'system'. */
  theme?: 'system' | 'light' | 'dark';
};

/**
 * Install a fake window.__TAURI_INTERNALS__ via page.addInitScript so that
 * the launcher SvelteKit app can mount in plain Chromium without a Rust
 * backend.  Call this before page.goto() / page.setContent().
 */
export async function installMockIpc(page: Page, state: MockState = {}): Promise<void> {
  await page.addInitScript((s) => {
    // Defaults — callers only need to supply the fields relevant to
    // the surface under test.
    const defaults: Required<typeof s> = {
      accounts: [],
      active_account_id: null,
      instances: [],
      active_instance_id: null,
      running: null,
      installing: false,
      theme: 'system',
    };
    const m = { ...defaults, ...s };

    // ---------------------------------------------------------------------------
    // Command handlers — keyed by the raw snake_case name that tauri-specta
    // passes to __TAURI_INTERNALS__.invoke().
    // ---------------------------------------------------------------------------
    type Handler = (args: Record<string, unknown>) => unknown;

    const handlers: Record<string, Handler> = {
      // Accounts
      list_accounts: () => m.accounts,
      get_active_account: () =>
        m.accounts.find((a: { id: string }) => a.id === m.active_account_id) ?? null,

      // Instances
      list_instances: () => m.instances,
      get_active_instance: () =>
        m.instances.find((i: { id: string }) => i.id === m.active_instance_id) ?? null,

      // App settings — returns a minimal AppFile_Serialize shape.
      // tour_completed_version MUST equal the app's TOUR_VERSION constant
      // (src/lib/onboarding/state.svelte.ts) so the first-run onboarding
      // tour is treated as already completed and never auto-fires. The
      // tour overlay disables pointer-events on the whole UI beneath it,
      // which breaks any click-driven interaction in tests. A non-matching
      // sentinel (e.g. 'skip') leaves the tour ACTIVE — keep this in sync if
      // TOUR_VERSION is bumped.
      app_settings_get: () => ({
        version: 1,
        active_instance: m.active_instance_id,
        onboarding: { tour_completed_version: '0.5.0' },
        general: {
          hide_to_tray_during_game: false,
          theme: m.theme,
        },
      }),

      // Version manifest — return empty list; UI guards on versionsError.
      list_versions: () => [],

      // Per-instance commands that fire immediately on instance switch.
      mods_list_installed: () => [],
      modpack_status: () => null,
      get_playtime: () => ({
        total_seconds: 0,
        session_count: 0,
        last_session_seconds: 0,
        last_session_unix_ms: null,
      }),

      // CurseForge key status — reported as missing so the key banner
      // appears predictably in Settings tests.
      mods_get_curseforge_key_status: () => 'missing',

      // Mod cache size (Settings panel).
      mods_cache_size_bytes: () => 0,

      // Log files / diagnoser — return empty/null so the Logs popover
      // mounts without errors.
      list_log_files: () => [],
      diagnose_log: () => null,
      latest_crash: () => null,

      // Worlds (Worlds tab).
      list_worlds: () => [],

      // Onboarding — mark as completed so tours don't overlay snapshots.
      app_settings_mark_tour_completed: () => null,
      app_settings_set_general: () => null,

      // Default catch-all — any unknown command returns null.
      __default: () => null,
    };

    // ---------------------------------------------------------------------------
    // Event system stub — @tauri-apps/api/event calls
    // `invoke('plugin:event|listen', ...)` and uses `transformCallback` to
    // register a numeric handler id.  We return a stable id of 0 and expose
    // `transformCallback` as a no-op so the module initialises cleanly.
    // listen() resolves to an unlisten no-op so the app can call it safely.
    // ---------------------------------------------------------------------------
    (
      window as Window & { __TAURI_INTERNALS__: unknown }
    ).__TAURI_INTERNALS__ = {
      invoke: async (cmdName: string, _args?: unknown): Promise<unknown> => {
        // Strip the "plugin:event|" prefix used by the event system, or
        // any other "plugin:X|" prefix that Tauri 2 plugins use.
        const key = cmdName.includes('|') ? (cmdName.split('|').pop() ?? cmdName) : cmdName;

        if (key === 'listen' || key === 'unlisten') {
          // Event system IPC — return a no-op unlisten token.
          return 0;
        }

        const handler: Handler = (handlers[key] as Handler | undefined) ?? (handlers.__default as Handler);
        return handler(_args as Record<string, unknown>);
      },

      // transformCallback is used by @tauri-apps/api to register JS
      // callbacks that the Rust side calls.  In tests we never fire
      // events from Rust, so a counter returning unique ids is sufficient.
      transformCallback: (() => {
        let nextId = 1;
        return (_cb: unknown, _once: boolean): number => nextId++;
      })(),
    };
  }, state);
}
