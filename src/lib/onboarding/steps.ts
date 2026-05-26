// Tour step content for v0.5.0 onboarding. `state.svelte.ts` derives
// TOTAL_STEPS from this array's length.
//
// Each step has a targetSelector that resolves to a UI element via
// document.querySelector at render time. `null` selector = centered
// modal (no spotlight) — used for the welcome step.
//
// anchor = 'right' | 'below' | 'center' is the popover side relative
// to the spotlight rect. Centered modal ignores anchor.

export type TourAnchor = 'right' | 'below' | 'center';

export interface TourStep {
  title: string;
  body: string;
  targetSelector: string | null;
  anchor: TourAnchor;
}

export const STEPS: ReadonlyArray<TourStep> = [
  {
    title: 'Welcome to FTlauncher',
    body: 'FTlauncher organises Minecraft into instances — self-contained worlds with their own version, loader, mods, configs and saves. Switching instance = switching Minecraft install, without touching the others.',
    targetSelector: null,
    anchor: 'center',
  },
  {
    title: 'Pick an instance',
    body: 'An instance is a self-contained Minecraft install. FTlauncher starts you with one called Default; this dropdown switches the active instance — Play, mods and logs all follow whichever one is selected.',
    targetSelector: '[data-tour="instance-picker"]',
    anchor: 'right',
  },
  {
    title: 'Manage your instances',
    body: "⚙ Manage is where you set an instance's Minecraft version and loader, tweak memory, and create, rename or delete instances. A new instance starts with no version — pick one here before you can play.",
    targetSelector: '[data-tour="manage-btn"]',
    anchor: 'right',
  },
  {
    title: 'Install, then Play',
    body: 'First click downloads Minecraft and your chosen loader (one-time per version). The button then switches to Play. While Minecraft is running it becomes Stop.',
    targetSelector: '[data-tour="play-btn"]',
    anchor: 'right',
  },
  {
    title: 'Browse mods',
    body: "Search Modrinth or CurseForge and install mods directly into the active instance's mods folder. SHA-verified, listed in the Installed sub-tab.",
    targetSelector: '[data-tour="tab-mods"]',
    anchor: 'below',
  },
  {
    title: 'Import modpacks',
    body: 'Drop a .mrpack or CurseForge .zip here, or browse Modrinth modpacks. Each import becomes a new instance — your existing instances stay untouched.',
    targetSelector: '[data-tour="open-modpacks"]',
    anchor: 'right',
  },
];
