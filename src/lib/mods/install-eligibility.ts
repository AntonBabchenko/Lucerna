// A mod can be installed into an instance only when an instance is
// selected and it has a non-vanilla loader — vanilla instances run no
// mods. MainTabs' drag-drop router and ModBrowserTab (its "Install from
// file…" button + the droppedMods consumer) share this one rule so the
// gating logic lives in a single place.

type Loader = 'vanilla' | 'fabric' | 'quilt' | 'forge' | 'neoforge' | null;

export function canInstallMods(instanceId: string | null, loader: Loader): boolean {
  return instanceId !== null && loader !== 'vanilla' && loader !== null;
}
