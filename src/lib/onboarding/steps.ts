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
import { DISCLAIMER_TEXT } from '../settings/disclaimer';

export type TourAnchor = 'right' | 'below' | 'center';

export interface TourStep {
  titleKey: TranslationKey;
  bodyKey: TranslationKey;
  targetSelector: string | null;
  anchor: TourAnchor;
  /** Optional small-print footer rendered under the body. Currently used
   *  on the welcome step to surface the Mojang Usage Guidelines
   *  disclaimer on first launch. */
  disclaimer?: string;
}

export const STEPS: ReadonlyArray<TourStep> = [
  {
    titleKey: 'onboarding.tour.welcome.title',
    bodyKey: 'onboarding.tour.welcome.body',
    targetSelector: null,
    anchor: 'center',
    disclaimer: DISCLAIMER_TEXT,
  },
  {
    titleKey: 'onboarding.tour.signIn.title',
    bodyKey: 'onboarding.tour.signIn.body',
    targetSelector: '[data-tour="ms-signin-btn"]',
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
