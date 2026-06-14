import type { ContentKind, ModSource } from '$lib/ipc/bindings';

// Cross-component state for opening the Settings modal at a specific tab.
//
// Pattern: a `.svelte.ts` module exporting a `$state(...)` rune is the
// v0.5.0 sub-3 way to share writable reactive state across unrelated
// components without a context tree. ModBrowseView writes
// `settingsOpen.value = { tab: 'curseforge' }`; the SettingsModal
// (lands in a later task) reads the same value to know whether to mount
// and which tab to focus.

export type SettingsTab = 'curseforge' | 'storage' | 'about' | 'general';

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
export type ModBrowserNav = { view: 'browse' | 'installed' };
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
