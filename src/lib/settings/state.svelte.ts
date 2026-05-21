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

// True while an OS file-drag is hovering an accepting tab. MainTabs'
// drag-drop listener sets it; FileDropzone reads it to show its drag
// highlight.
export const dragActive = $state<{ value: boolean }>({ value: false });
