// Tour step content for v0.5.0 onboarding. `state.svelte.ts` derives
// TOTAL_STEPS from this array's length.
//
// Each step has a targetSelector that resolves to a UI element via
// document.querySelector at render time. `null` selector = centered
// modal (no spotlight) — used for the welcome step.
//
// anchor = 'right' | 'below' | 'center' is the popover side relative
// to the spotlight rect. Centered modal ignores anchor.
//
// titleKey / bodyKey are TranslationKey references resolved at render
// time via $t(step.titleKey). The actual text lives in
// src/lib/i18n/locales/{en,ru}.json under onboarding.tour.*.

import type { TranslationKey } from '../i18n/keys.generated';

export type TourAnchor = 'right' | 'below' | 'center';

export interface TourStep {
  titleKey: TranslationKey;
  bodyKey: TranslationKey;
  targetSelector: string | null;
  anchor: TourAnchor;
  /** Optional small-print footer rendered under the body. Currently used
   *  on the welcome step to surface the Mojang Usage Guidelines
   *  disclaimer on first launch. Resolved via $t at render time. */
  disclaimerKey?: TranslationKey;
  /** Discriminator for special steps. `'chooser'` renders the Basic/Advanced
   *  picker instead of normal title/body + Back/Skip/Next, and its title/body
   *  keys are used verbatim (NOT run through explainKey). */
  kind?: 'chooser';
}

export const STEPS: ReadonlyArray<TourStep> = [
  {
    kind: 'chooser',
    titleKey: 'onboarding.chooser.title',
    bodyKey: 'onboarding.chooser.body',
    targetSelector: null,
    anchor: 'center',
  },
  {
    titleKey: 'onboarding.tour.welcome.title',
    bodyKey: 'onboarding.tour.welcome.body',
    targetSelector: null,
    anchor: 'center',
    disclaimerKey: 'settings.about.disclaimer',
  },
  {
    // Spotlights the whole ACCOUNT section (account select + "Add offline" +
    // Microsoft sign-in), not just the Microsoft button — both paths to a
    // playable account are equal first-class options. This same step is reused
    // on demand as the "you need an account" hint when Play is clicked with no
    // active account (see showAccountHint in state.svelte.ts).
    titleKey: 'onboarding.tour.signIn.title',
    bodyKey: 'onboarding.tour.signIn.body',
    targetSelector: '[data-tour="account-section"]',
    anchor: 'right',
  },
  {
    titleKey: 'onboarding.tour.pickInstance.title',
    bodyKey: 'onboarding.tour.pickInstance.body',
    targetSelector: '[data-tour="instance-picker"]',
    anchor: 'right',
  },
  {
    titleKey: 'onboarding.tour.manageInstances.title',
    bodyKey: 'onboarding.tour.manageInstances.body',
    targetSelector: '[data-tour="manage-btn"]',
    anchor: 'right',
  },
  {
    titleKey: 'onboarding.tour.installPlay.title',
    bodyKey: 'onboarding.tour.installPlay.body',
    targetSelector: '[data-tour="play-btn"]',
    anchor: 'right',
  },
  {
    titleKey: 'onboarding.tour.browseMods.title',
    bodyKey: 'onboarding.tour.browseMods.body',
    targetSelector: '[data-tour="tab-mods"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.tour.importModpacks.title',
    bodyKey: 'onboarding.tour.importModpacks.body',
    targetSelector: '[data-tour="open-modpacks"]',
    anchor: 'right',
  },
];
