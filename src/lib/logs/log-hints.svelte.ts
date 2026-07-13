// Pure helpers + hover-state composable for inline log error hints.
// The pure functions are unit-tested; the composable holds the open/close
// timing rules (open delay, hover-bridge grace) shared by the client log
// viewer and the server console. No DOM globals at module level.

import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { AnnotateResult } from '$lib/ipc/bindings';
import { DIAGNOSIS_COPY } from '$lib/logs/diagnosis-copy';

export interface HintCopy {
  title: string;
  explanation: string;
  recommendation: string;
}

/** line index (original, 0-based) → pattern id */
export function annotationMap(result: AnnotateResult | null): Map<number, string> {
  const map = new Map<number, string>();
  if (!result) return map;
  for (const a of result.annotations) map.set(a.line, a.pattern_id);
  return map;
}

/** pattern id → English fallback copy shipped in the command response */
export function fallbackCopyMap(result: AnnotateResult | null): Map<string, HintCopy> {
  const map = new Map<string, HintCopy>();
  if (!result) return map;
  for (const p of result.patterns) {
    map.set(p.id, {
      title: p.title,
      explanation: p.explanation,
      recommendation: p.recommendation,
    });
  }
  return map;
}

/**
 * Localized copy for a pattern: prefers the i18n keys in DIAGNOSIS_COPY,
 * falls back to the English strings the backend shipped. `translate` is
 * the `$t` store value so this stays a pure function.
 */
export function resolveHintCopy(
  patternId: string,
  fallbacks: Map<string, HintCopy>,
  translate: (key: TranslationKey) => string,
): HintCopy | null {
  const keys = DIAGNOSIS_COPY[patternId];
  if (keys) {
    return {
      title: translate(keys.title),
      explanation: translate(keys.explanation),
      recommendation: translate(keys.recommendation),
    };
  }
  return fallbacks.get(patternId) ?? null;
}

export interface ActiveHint {
  patternId: string;
  copy: HintCopy;
  /** Trigger rect snapshot (fixed-position coordinates). */
  anchor: { top: number; left: number; width: number; height: number; bottom: number };
}

const OPEN_DELAY_MS = 300;
const GRACE_MS = 150;

/**
 * Hover controller for one surface. Row handlers arm a delayed open;
 * leaving row or card arms a short grace close so the pointer can travel
 * onto the card (hover bridge). Keyboard focus opens instantly (callers
 * gate on :focus-visible). Call `dispose()` in onDestroy.
 */
export function createLogHintHover() {
  let active = $state<ActiveHint | null>(null);
  let openTimer: ReturnType<typeof setTimeout> | null = null;
  let closeTimer: ReturnType<typeof setTimeout> | null = null;

  function clearTimers() {
    if (openTimer) clearTimeout(openTimer);
    if (closeTimer) clearTimeout(closeTimer);
    openTimer = null;
    closeTimer = null;
  }

  function show(hint: ActiveHint) {
    clearTimers();
    active = hint;
  }

  function cancelClose() {
    if (closeTimer) {
      clearTimeout(closeTimer);
      closeTimer = null;
    }
  }

  function armClose() {
    if (openTimer) {
      clearTimeout(openTimer);
      openTimer = null;
    }
    if (active) {
      closeTimer = setTimeout(() => {
        active = null;
        closeTimer = null;
      }, GRACE_MS);
    }
  }

  return {
    get active() {
      return active;
    },
    rowEnter(hint: ActiveHint) {
      cancelClose();
      if (active) {
        // Already open: retarget immediately (moving between annotated rows).
        show(hint);
        return;
      }
      if (openTimer) clearTimeout(openTimer);
      openTimer = setTimeout(() => show(hint), OPEN_DELAY_MS);
    },
    rowLeave() {
      armClose();
    },
    cardEnter() {
      cancelClose();
    },
    cardLeave() {
      armClose();
    },
    openFromFocus(hint: ActiveHint) {
      show(hint);
    },
    close() {
      clearTimers();
      active = null;
    },
    dispose() {
      clearTimers();
      active = null;
    },
  };
}

export type LogHintHover = ReturnType<typeof createLogHintHover>;
