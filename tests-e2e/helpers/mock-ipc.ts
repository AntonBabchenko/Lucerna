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

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Page } from '@playwright/test';

// The app's onboarding tour version, derived from the single source of truth
// (`export const TOUR_VERSION = '…'` in src/lib/onboarding/state.svelte.ts)
// rather than hard-coded here. That module can't be imported directly: it is a
// Svelte runes module that pulls in `$lib` aliases Playwright's tsconfig does
// not resolve. So we read the source text and extract the literal. If the
// constant is ever renamed or removed, this throws LOUDLY at test-setup time
// instead of silently pinning a stale version (which would leave the first-run
// tour ACTIVE and break every click-driven e2e). Keeping it derived means a
// TOUR_VERSION bump can never silently desync the mock.
function resolveTourVersion(): string {
  const here = dirname(fileURLToPath(import.meta.url));
  const statePath = join(here, '../../src/lib/onboarding/state.svelte.ts');
  const src = readFileSync(statePath, 'utf8');
  const match = src.match(/export\s+const\s+TOUR_VERSION\s*=\s*['"]([^'"]+)['"]/);
  if (!match) {
    throw new Error(
      `mock-ipc: could not find "export const TOUR_VERSION = '…'" in ${statePath}. ` +
        'The onboarding tour-version source of truth moved or was renamed — update ' +
        'resolveTourVersion() so the mock IPC keeps reporting the tour as completed.',
    );
  }
  return match[1];
}

// The per-surface *contextual* tours (ContextualTour.svelte) are gated by
// versioned localStorage keys (`ftl.tour.<id>.<version>.done`), independent of
// the main tour above. In a fresh browser those keys are absent, so opening any
// toured surface (Manage, Add-ons, …) auto-fires its tour and its popover can
// sit over the control a click-driven spec is trying to reach. Seed every key
// as "done" so contextual tours never fire in e2e. Derived from the app's
// `TOUR_VERSION` map (same don't-hard-code rationale as resolveTourVersion) so a
// version bump can't silently desync and re-break the specs.
function resolveContextualTourKeys(): string[] {
  const here = dirname(fileURLToPath(import.meta.url));
  const p = join(here, '../../src/lib/onboarding/contextual-tours.ts');
  const src = readFileSync(p, 'utf8');
  const block = src.match(/const TOUR_VERSION\s*:[^{]*\{([\s\S]*?)\}/);
  if (!block) {
    throw new Error(
      `mock-ipc: could not find the contextual TOUR_VERSION map in ${p}. ` +
        'The contextual-tour version source moved or was renamed — update ' +
        'resolveContextualTourKeys() so e2e keeps suppressing contextual tours.',
    );
  }
  const keys: string[] = [];
  for (const m of block[1].matchAll(/(\w+)\s*:\s*['"]([^'"]+)['"]/g)) {
    keys.push(`ftl.tour.${m[1]}.${m[2]}.done`);
  }
  if (keys.length === 0) {
    throw new Error(`mock-ipc: contextual TOUR_VERSION map parsed empty in ${p}.`);
  }
  return keys;
}

const TOUR_VERSION = resolveTourVersion();
const CONTEXTUAL_TOUR_KEYS = resolveContextualTourKeys();

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

// A mod search hit (ModSummary subset the browse cards read).
export type MockModSummary = {
  source: 'modrinth' | 'curseforge';
  project_id: string;
  slug: string | null;
  name: string;
  summary: string;
  icon_url: string | null;
  downloads: number | null;
  author: string;
  updated_at: string | null;
};

// An installed-mod row (InstalledMod subset). mods_install_with_deps appends
// one here so a later mods_list_installed flips the browse card to "Installed".
export type MockInstalledMod = {
  filename: string;
  sha1: string;
  source: 'modrinth' | 'curseforge' | null;
  project_id: string | null;
  version_id: string | null;
  name: string;
  version_number: string | null;
  installed_at: number;
  enabled: boolean;
  requires: string[];
  enrich_attempted: boolean;
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
  /** Hits returned by mods_search; defaults to empty. */
  mod_hits?: MockModSummary[];
  /**
   * Installed mods returned by mods_list_installed. Seeded empty and APPENDED
   * to by mods_install_with_deps so the browse flow's post-install refresh sees
   * the new install. Defaults to empty.
   */
  installed_mods?: MockInstalledMod[];
};

/**
 * Build a MockInstance with sensible defaults, overriding only the fields a
 * test cares about. Centralising the default shape here means a new REQUIRED
 * field on MockInstance is added in ONE place — callers that don't care about
 * it don't need editing, instead of hand-patching every literal across the
 * spec suite.
 *
 * Defaults describe a ready vanilla instance with no modpack provenance.
 */
export function makeInstance(overrides: Partial<MockInstance> = {}): MockInstance {
  return {
    id: 'inst-1',
    name: 'Test Instance',
    mc_version: '1.20.4',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: true,
    mrpack_name: null,
    mrpack_version: null,
    mrpack_project_id: null,
    mrpack_source: null,
    mrpack_summary: null,
    mrpack_version_id: null,
    ...overrides,
  };
}

/**
 * Build a MockInstalledMod with sensible defaults, overriding only the fields a
 * test cares about. Same rationale as makeInstance: a new REQUIRED field lands
 * in one place instead of forcing edits across every installed-mod literal.
 *
 * Defaults describe an enabled Modrinth mod with no dependencies.
 */
export function makeInstalledMod(overrides: Partial<MockInstalledMod> = {}): MockInstalledMod {
  return {
    filename: 'mod.jar',
    sha1: 'sha-mod',
    source: 'modrinth',
    project_id: 'mod',
    version_id: 'mod-v1',
    name: 'Test Mod',
    version_number: '1.0.0',
    installed_at: 0,
    enabled: true,
    requires: [],
    enrich_attempted: false,
    ...overrides,
  };
}

/**
 * Install a fake window.__TAURI_INTERNALS__ via page.addInitScript so that
 * the launcher SvelteKit app can mount in plain Chromium without a Rust
 * backend.  Call this before page.goto() / page.setContent().
 */
export async function installMockIpc(page: Page, state: MockState = {}): Promise<void> {
  // The init script runs in the browser, where the Node-side TOUR_VERSION const
  // is out of scope — so pass it through as serialized data alongside the state.
  await page.addInitScript(
    (arg: { state: MockState; tourVersion: string; contextualTourKeys: string[] }) => {
      const s = arg.state;
      const tourVersion = arg.tourVersion;
      // Suppress every per-surface contextual tour so its popover never sits
      // over a control a spec is clicking. Runs before the app mounts.
      try {
        for (const k of arg.contextualTourKeys) localStorage.setItem(k, '1');
      } catch {
        /* localStorage unavailable (private mode) — acceptable, tours are non-blocking */
      }
      // Defaults — callers only need to supply the fields relevant to
      // the surface under test.
      const defaults: Required<MockState> = {
        accounts: [],
        active_account_id: null,
        instances: [],
        active_instance_id: null,
        running: null,
        installing: false,
        theme: 'system',
        mod_hits: [],
        installed_mods: [],
      };
      const m = { ...defaults, ...s };

      // -------------------------------------------------------------------------
      // Command handlers — keyed by the raw snake_case name that tauri-specta
      // passes to __TAURI_INTERNALS__.invoke().
      // -------------------------------------------------------------------------
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
        // sentinel (e.g. 'skip') leaves the tour ACTIVE. The value is derived
        // from the app's TOUR_VERSION source (see resolveTourVersion above) and
        // passed in as `tourVersion`, so a version bump can never silently
        // desync the mock.
        app_settings_get: () => ({
          version: 1,
          active_instance: m.active_instance_id,
          onboarding: { tour_completed_version: tourVersion },
          general: {
            hide_to_tray_during_game: false,
            theme: m.theme,
          },
        }),

        // Version manifest — return empty list; UI guards on versionsError.
        list_versions: () => [],

        // Per-instance commands that fire immediately on instance switch.
        // Reflects installs recorded by mods_install_with_deps (defaults []).
        mods_list_installed: () => m.installed_mods,
        modpack_status: () => null,

        // Mod browser — search + dependency-free fast-path install. Lets an e2e
        // drive the "find a mod → install it → card shows Installed" core flow.
        mods_search: () => ({ hits: m.mod_hits, total: m.mod_hits.length }),
        // A card "Install" (no pinned version) first fetches the project's
        // versions and installs the newest. Return one fabric-compatible version
        // per requested project so the fast-path (no deps, no loader mismatch)
        // applies.
        mods_versions: (args) => {
          const a = args as { source?: 'modrinth' | 'curseforge'; projectId?: string };
          const pid = a.projectId ?? 'mod';
          return [
            {
              source: a.source ?? 'modrinth',
              project_id: pid,
              version_id: `${pid}-v1`,
              name: `${pid} 1.0.0`,
              version_number: '1.0.0',
              game_versions: ['1.20.4'],
              loaders: ['fabric'],
              primary_file: { filename: `${pid}.jar`, url: '', sha1: '', size: 0 },
              dependencies: [],
            },
          ];
        },
        mods_resolve_install_plan: () => ({
          required: [],
          optional: [],
          incompatible: [],
          unresolvable: [],
          loader_requirements: [],
        }),
        mods_install_with_deps: (args) => {
          const primary = (args?.primary ?? {}) as {
            source?: 'modrinth' | 'curseforge';
            project_id?: string;
            version_id?: string;
          };
          const hit = m.mod_hits.find(
            (h) => h.source === primary.source && h.project_id === primary.project_id,
          );
          const name = hit?.name ?? 'Installed mod';
          m.installed_mods.push({
            filename: `${primary.project_id ?? 'mod'}.jar`,
            sha1: `sha-${primary.project_id ?? 'mod'}`,
            source: primary.source ?? null,
            project_id: primary.project_id ?? null,
            version_id: primary.version_id ?? null,
            name,
            version_number: '1.0.0',
            installed_at: 0,
            enabled: true,
            requires: [],
            enrich_attempted: false,
          });
          return { primary_name: name, installed_dependencies: [] };
        },
        // refreshInstalled() looks up each installed mod's display name.
        mods_project: (args) => ({
          summary: {
            name: `Project ${(args as { project_id?: string })?.project_id ?? ''}`,
            slug: null,
          },
        }),
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

      // -------------------------------------------------------------------------
      // Event system stub — @tauri-apps/api/event calls
      // `invoke('plugin:event|listen', ...)` and uses `transformCallback` to
      // register a numeric handler id.  We return a stable id of 0 and expose
      // `transformCallback` as a no-op so the module initialises cleanly.
      // listen() resolves to an unlisten no-op so the app can call it safely.
      // -------------------------------------------------------------------------
      (window as Window & { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
        invoke: async (cmdName: string, _args?: unknown): Promise<unknown> => {
          // Strip the "plugin:event|" prefix used by the event system, or
          // any other "plugin:X|" prefix that Tauri 2 plugins use.
          const key = cmdName.includes('|') ? (cmdName.split('|').pop() ?? cmdName) : cmdName;

          if (key === 'listen' || key === 'unlisten') {
            // Event system IPC — return a no-op unlisten token.
            return 0;
          }

          const handler: Handler =
            (handlers[key] as Handler | undefined) ?? (handlers.__default as Handler);
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
    },
    { state, tourVersion: TOUR_VERSION, contextualTourKeys: CONTEXTUAL_TOUR_KEYS },
  );
}
