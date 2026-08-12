import { describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(async () => ({ status: 'error', error: 'unused' })),
    appSettingsMarkTourCompleted: vi.fn(async () => ({ status: 'ok', data: null })),
  },
}));

import en from '../src/lib/i18n/locales/en.json';
import {
  ALL_CONTEXTUAL_TOUR_IDS,
  fingerprintSteps,
  fnv1a,
  STEPS_BY_ID,
  STEPS_FINGERPRINT,
} from '../src/lib/onboarding/contextual-tours';
import { MAIN_STEPS_FINGERPRINT } from '../src/lib/onboarding/state.svelte';
import { STEPS, type TourStep } from '../src/lib/onboarding/steps';

// The bump nudge (#294): editing a tour's steps must force an edit on the
// fingerprint line that sits BESIDE the TOUR_VERSION line, so author and
// reviewer both face the "does this need a version bump?" question.
//
// The hash covers STRUCTURE *and COPY*. Structure alone has a blind spot that
// defeats half the purpose: the worlds v3→v4 bump was justified entirely by an
// en.json edit with WORLDS_STEPS byte-identical, and #294 itself was a
// re-anchor PLUS a copy rewrite — structure alone catches only the re-anchor.
// Resolving the keys HERE rather than in contextual-tours.ts keeps production
// code free of locale imports.
//
// EN ONLY, deliberately: RU belongs to Weblate, and a translation sync must
// never redden CI.

type Json = Record<string, unknown>;

/** Walk a dotted key path through the locale JSON — same traversal as
 *  tests/explanation-basic-parity.test.ts. */
function getKey(obj: Json, dotted: string): unknown {
  return dotted.split('.').reduce<unknown>((acc, part) => {
    if (acc && typeof acc === 'object') return (acc as Json)[part];
    return undefined;
  }, obj);
}

/** Every EN string a step renders: the Advanced value at each key plus its
 *  `Basic` sibling (see explainKey). Absent siblings resolve to null rather
 *  than being skipped — chooser steps and the welcome disclaimer have no Basic
 *  variant, and a stable null keeps their slot in the hash. */
function resolveCopy(steps: ReadonlyArray<TourStep>, locale: Json): (string | null)[] {
  const out: (string | null)[] = [];
  for (const step of steps) {
    for (const key of [step.titleKey, step.bodyKey, step.disclaimerKey]) {
      if (!key) continue;
      for (const k of [key, `${key}Basic`]) {
        const v = getKey(locale, k);
        out.push(typeof v === 'string' ? v : null);
      }
    }
  }
  return out;
}

/** Structure + resolved EN copy, folded through the one FNV-1a in
 *  contextual-tours.ts so there is a single hash implementation to drift. */
function fingerprintTour(steps: ReadonlyArray<TourStep>, locale: Json = en as Json): string {
  return fnv1a(`${fingerprintSteps(steps)} ${JSON.stringify(resolveCopy(steps, locale))}`);
}

describe('tour steps fingerprints', () => {
  for (const id of ALL_CONTEXTUAL_TOUR_IDS) {
    it(`${id}: recorded fingerprint matches the steps and their EN copy`, () => {
      expect(
        STEPS_FINGERPRINT[id],
        `steps or EN copy for "${id}" changed: set STEPS_FINGERPRINT.${id} to "${fingerprintTour(STEPS_BY_ID[id])}" AND decide whether TOUR_VERSION.${id} must bump (see the bump contract comment)`,
      ).toBe(fingerprintTour(STEPS_BY_ID[id]));
    });
  }

  it('main tour: recorded fingerprint matches STEPS and their EN copy', () => {
    expect(
      MAIN_STEPS_FINGERPRINT,
      `main STEPS or their EN copy changed: set MAIN_STEPS_FINGERPRINT to "${fingerprintTour(STEPS)}" AND decide whether the main TOUR_VERSION must bump`,
    ).toBe(fingerprintTour(STEPS));
  });

  it('a copy-only edit moves the fingerprint even though the structure is unchanged', () => {
    // Locks the fix for the structure-only blind spot. Without this, a future
    // refactor could quietly revert to hashing structure alone and every
    // recorded value above would still match — the nudge would rot silently.
    const steps = STEPS_BY_ID.worlds;
    const doctored = JSON.parse(JSON.stringify(en)) as Json;
    const path = 'onboarding.contextual.worlds.worldsWithBackups';
    const leaf = getKey(doctored, path) as Json;
    leaf.body = `${String(leaf.body)} (edited)`;

    // Structure is untouched by definition — the step objects never moved.
    expect(fingerprintSteps(steps)).toBe(fingerprintSteps(STEPS_BY_ID.worlds));
    // ...yet the fingerprint moves, so the bump question still gets asked.
    expect(fingerprintTour(steps, doctored)).not.toBe(fingerprintTour(steps));
  });

  it('a Basic-sibling edit also moves the fingerprint', () => {
    // The Basic register is real shipped copy; a rewrite there is as material
    // as one in the Advanced register.
    const steps = STEPS_BY_ID.worlds;
    const doctored = JSON.parse(JSON.stringify(en)) as Json;
    const leaf = getKey(doctored, 'onboarding.contextual.worlds.worldsWithBackups') as Json;
    leaf.bodyBasic = `${String(leaf.bodyBasic)} (edited)`;

    expect(fingerprintTour(steps, doctored)).not.toBe(fingerprintTour(steps));
  });
});
