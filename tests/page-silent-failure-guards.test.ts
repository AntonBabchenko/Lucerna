/**
 * `+page.svelte` is the whole app shell: it is not renderable under vitest (and
 * is excluded from the coverage denominator in vitest.config.js for exactly
 * that reason), so these are source-scan guards — the same shape as
 * `tests/external-change-no-relist.test.ts` and `tools/check-no-network-calls.mjs`.
 *
 * What they pin: boot-path IPC reads that used to have an ok-only branch, so a
 * failure produced a confident wrong screen (defaults / no feedback / an empty
 * list) instead of a report.
 *
 * HONEST LIMIT: a structural guard proves the branch exists and names the right
 * keys. It cannot prove the toast renders — that half is owed to the dev smoke.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const PAGE = resolve('src/routes/+page.svelte');
const src = readFileSync(PAGE, 'utf8');

/**
 * The body of a function, brace-matched from its header. Adequate here because
 * none of the bodies scanned contains a brace inside a string or a template
 * literal; if one ever does, this returns a short slice and the assertion below
 * fails loudly rather than passing by accident.
 */
function functionBody(header: string): string {
  const start = src.indexOf(header);
  expect(start, `${header} must still exist in +page.svelte`).toBeGreaterThan(-1);
  const open = src.indexOf('{', start);
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    if (src[i] === '{') depth++;
    else if (src[i] === '}' && --depth === 0) return src.slice(open, i + 1);
  }
  throw new Error(`unbalanced braces after ${header}`);
}

describe('startup settings read', () => {
  it('surfaces a failed read instead of silently running on defaults', () => {
    const body = functionBody('async function loadStartupSettings()');
    // A failure must reach the user, and must offer the retry — the whole point
    // is that the settings on disk are fine and one more read may succeed.
    expect(body).toContain('pushActionToast');
    expect(body).toContain('page.startupSettings.loadFailed');
    expect(body).toContain('page.startupSettings.retry');
    expect(body).toContain('loadStartupSettings()');
  });

  it('does not re-register the theme media listener on a retry', () => {
    // initTheme() returns an unlistener; a retryable loader that drops it stacks
    // a second prefers-color-scheme listener on every attempt.
    const body = functionBody('function applyStartupSettings(');
    expect(body).toContain('themeUnlisten?.()');
    expect(body).toContain('themeUnlisten = initTheme(');
    expect(src, 'the held unlistener must also be released on unmount').toContain(
      'themeUnlisten?.();',
    );
  });
});

describe('account switch', () => {
  it('reports a failed switch instead of leaving the picker unchanged', () => {
    const body = functionBody('async function onSelectAccount(id: string)');
    // The Select is controlled off `activeAccount`, so on failure the UI does
    // not move at all — the only signal available is an explicit one.
    expect(body).toContain('else');
    expect(body).toContain('pushWarning');
    expect(body).toContain('page.accounts.switchFailed');
    expect(body).toContain('formatError(result.error)');
  });
});
