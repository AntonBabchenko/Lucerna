import type { ContentKind, ModSource } from '$lib/ipc/bindings';

// Cross-component state for opening the Settings modal at a specific tab.
//
// Pattern: a `.svelte.ts` module exporting a `$state(...)` rune is the
// v0.5.0 sub-3 way to share writable reactive state across unrelated
// components without a context tree. ModBrowseView writes
// `settingsOpen.value = { tab: 'integrations' }`; the SettingsModal reads
// the same value to know whether to mount and which section to focus.

export type SettingsTab =
  | 'appearance'
  | 'game'
  | 'integrations'
  | 'storage'
  | 'updates'
  | 'help'
  | 'about';

export const settingsOpen = $state<{ value: { tab: SettingsTab } | null }>({
  value: null,
});

// Tick that increments whenever the CurseForge API key changes (saved
// or cleared). Watchers that gate UI on the key's existence — e.g.
// ModBrowseView's CurseForge banner — read this rune to know when to
// re-poll mods_get_curseforge_key_status. Lets the key form in
// Settings notify the rest of the UI without prop drilling.
export const cfKeyVersion = $state<{ value: number }>({ value: 0 });

// Cross-component navigation into the Mod browser tab. The Overview
// "Installed mods" link sets this; MainTabs flips to mod_browser and
// ModBrowserTab honours the requested sub-view, then resets the rune
// to null so subsequent in-tab clicks don't get hijacked.
// `filter` deep-links a status view of the Installed list. The Overview's
// "N incompatible mods" indicator sets it: navigating to 140 unfiltered rows
// left the user with no way to tell WHICH mods the warning meant.
export type ModBrowserNav = {
  view: 'browse' | 'installed';
  filter?: 'incompatible';
};
export const modBrowserNav = $state<{ value: ModBrowserNav | null }>({ value: null });

// Deep-link request to open a specific project's detail modal in the Mod
// browser. The Add-ons → Shaders loader hint sets this to open Iris: it flips
// the Add-ons tab to the Mods segment (which re-keys ModBrowseView) and the
// freshly-mounted mod browser consumes the rune, opens the detail modal, and
// resets it to null. Mirrors `modBrowserNav`. The consumer guards on `isMod`
// + a source match so the about-to-unmount shader browser never steals it.
export type ModBrowseOpenProject = { source: ModSource; projectId: string };
export const modBrowseOpenProject = $state<{ value: ModBrowseOpenProject | null }>({
  value: null,
});

// Cross-component navigation into the Modpacks tab's imported-pack
// drawer. The Overview "missing mods" indicator sets this; MainTabs
// flips to the Modpacks tab, ModpacksTab flips to its Imported sub-tab,
// and ImportedView opens the drawer for the named instance, then
// resets the rune. Mirrors `modBrowserNav`.
export type ModpacksNav = { openDrawerForInstance: string };
export const modpacksNav = $state<{ value: ModpacksNav | null }>({ value: null });

// Files dropped onto the Mods tab, routed here by MainTabs' single
// window-level drag-drop listener. Absolute `.jar` paths. ModBrowserTab
// consumes this and resets it to null. Mirrors `modBrowserNav`.
export const droppedMods = $state<{ value: string[] | null }>({ value: null });
// A modpack file (`.mrpack`/`.zip`) dropped onto the Modpacks tab,
// routed here by MainTabs. ModpacksTab consumes this and resets it.
export const droppedModpack = $state<{ value: string | null }>({ value: null });

// The Add-ons tab's currently active content kind, published so MainTabs'
// single window-level drag-drop listener can route by kind: a `.jar` drop only
// makes sense on the Mods segment, a `.zip` drop on the Resource-pack/Shader
// segments. AddonsTab writes this on mount + kind change and resets it to 'mod'
// on destroy.
export const addonsKind = $state<{ value: ContentKind }>({ value: 'mod' });

// Local `.zip` files dropped onto the Add-ons tab while a Resource-pack or
// Shader segment is active, routed here by MainTabs. AddonsTab consumes this
// (guarding that `kind` still matches its active segment) and resets it to null.
// Mirrors `droppedMods`.
export const droppedAssets = $state<{ value: { kind: ContentKind; paths: string[] } | null }>({
  value: null,
});

// Paths (a `.zip` or a world folder) dropped onto the Worlds tab, routed here
// by MainTabs' single window-level drag-drop listener. WorldsTab consumes this
// and resets it to null. Mirrors `droppedMods`.
export const droppedWorld = $state<{ value: string[] | null }>({ value: null });

// MainTabs' active tab, mirrored for the window drop router in +page.svelte
// (the router must know whether the client is on Add-ons or Worlds).
export const clientActiveTab = $state<{ value: string }>({ value: 'overview' });

// ── Servers-mode add-ons drop routing ────────────────────────────────────────
// The content kind currently shown by the servers Add-ons tab ('mod' |
// 'plugin' | 'datapack'), mirrored by ServerAddonsTab while it is mounted and
// reset to null on destroy so a stale kind never poisons a future drop
// (same lifecycle contract as addonsKind above). Read by the window-level
// drop router in +page.svelte.
export type ServerAddonsKind = 'mod' | 'plugin' | 'datapack';
export const serverAddonsKind = $state<{ value: ServerAddonsKind | null }>({ value: null });

// Files dropped while servers mode is active and the Add-ons tab is shown;
// consumed (and cleared back to null) by the matching Add-ons pane. Distinct
// from droppedServer below, which is the server-IMPORT zip drop.
export const droppedServerContent = $state<{
  value: { kind: ServerAddonsKind; paths: string[] } | null;
}>({ value: null });

// A server import source — a `.zip` or a server folder — dropped onto the open
// Server-import view. Routed by the import view's OWN window-level listener (NOT
// MainTabs), so this is consumed there. Mirrors `droppedWorld`.
export const droppedServer = $state<{ value: string[] | null }>({ value: null });

// True while the Server-import view is mounted and owns drag-drop. MainTabs'
// window-level listener checks this and early-returns so a drop on the import
// modal isn't ALSO routed into the Worlds/Mods tabs underneath.
export const serverImportActive = $state<{ value: boolean }>({ value: false });

// True while an OS file-drag is hovering an accepting tab. MainTabs'
// drag-drop listener sets it; FileDropzone reads it to show its drag
// highlight.
export const dragActive = $state<{ value: boolean }>({ value: false });

// Bumped whenever a resource pack / shader is installed or uninstalled, so the
// Browse badges and the Installed-assets list stay in sync (assets have no
// Tauri events like mods do). Consumers read `.value` inside an $effect to
// refetch; producers call the bump after a successful install/uninstall.
// Producers must bump ONLY in action handlers — never inside a fetch effect,
// or the refetch would re-bump and loop.
export const assetsChanged = $state<{ value: number }>({ value: 0 });

// Mojang's Minecraft version list — fetched once at app startup
// (+page.svelte onMount) and consumed by both the McVersionCombobox in
// the mod / modpack browsers and the Manage modal's version picker.
// Stored in a rune so the leaf components don't need a 3+ level prop
// drill through MainTabs / ModpacksTab. Empty until startup fetch
// completes; combobox shows just the "Any version" entry until then.
import type { VersionEntry } from '$lib/ipc/bindings';
export const mcVersions = $state<{ value: VersionEntry[] }>({ value: [] });
