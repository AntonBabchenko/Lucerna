/**
 * `+page.svelte` owns the active profile's dependency pre-flight (spec §6.2, audit C-Q7): the
 * Overview reads its report and the Play gate runs its FRESH check through it. The page is the
 * whole app shell and is not renderable under vitest, so — like
 * `tests/external-change-no-relist.test.ts` and `tests/page-silent-failure-guards.test.ts` — these
 * are source-scan guards. They prove the wiring exists, not that it renders; the gate's own
 * behaviour is pinned by `tests/preflight-gate-dialog.test.ts` and `tests/fix-all.test.ts`.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const src = readFileSync(resolve('src/routes/+page.svelte'), 'utf8');

/**
 * The body of a function, brace-matched from its header. None of the bodies scanned contains a
 * brace inside a string or a template literal; if one ever does, this returns a short slice and
 * an assertion fails loudly rather than passing by accident.
 */
function functionBody(header: string): string {
  const start = src.indexOf(header);
  expect(start, `${header} must exist in +page.svelte`).toBeGreaterThan(-1);
  const open = src.indexOf('{', start);
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    if (src[i] === '{') depth++;
    else if (src[i] === '}' && --depth === 0) return src.slice(open, i + 1);
  }
  throw new Error(`unbalanced braces after ${header}`);
}

describe('+page pre-flight wiring', () => {
  it('owns one pre-flight for the active profile and releases it', () => {
    expect(src).toContain('createPreflight(() => activeInstance?.id ?? null)');
    expect(src).toContain('pagePreflight.dispose()');
  });

  it('still runs a FRESH pre-flight at Play — never the cached report', () => {
    const body = functionBody('async function startLaunch(');
    expect(body).toContain('pagePreflight.check(');
    expect(body).not.toContain('pagePreflight.report');
    expect(body).not.toContain('commands.instanceDependencyPreflight');
  });

  it('repairs, re-checks, and launches only through the shared gate', () => {
    const body = functionBody('async function onGateFixAndLaunch(');
    expect(body).toContain('repairForLaunch(');
    expect(body).toContain('pagePreflight.check(');
    expect(body).toContain('gateLaunch(');
    expect(src).not.toContain('remediateAll');
  });

  it('gives the gate the profile, so it can name dependencies from the name store', () => {
    const gate = src.slice(
      src.indexOf('<PreflightGateDialog'),
      src.indexOf('/>', src.indexOf('<PreflightGateDialog')),
    );
    expect(gate).toContain('instanceId={');
    expect(gate).toContain('onFixAndLaunch={onGateFixAndLaunch}');
    expect(gate).toContain('fixed={gateFixed}');
  });

  it('keeps the page pre-flight fresh on every mod event', () => {
    for (const name of [
      'debouncedModSetStats',
      'debouncedModToggleStats',
      'debouncedExternalChangeStats',
    ]) {
      const start = src.indexOf(`const ${name}`);
      expect(start, name).toBeGreaterThan(-1);
      expect(src.slice(start, src.indexOf('}, 150);', start))).toContain(
        'pagePreflight.invalidate()',
      );
    }
  });

  it('feeds the Overview from the page pre-flight and the persisted update check', () => {
    // Each mod's level by the rows' own statusOf (problemCounts), from the page pre-flight's
    // blocking rows and compat's reasons — the Overview and the Installed chip cannot disagree.
    expect(src).toContain('problemCounts(');
    expect(src).toContain('blockingModsCount={modProblems.blocking}');
    expect(src).toContain('incompatibleCount={modProblems.warning}');
    expect(src).toContain('problemCount={modsProblemCount}');
    expect(src).toContain('updateCount={stats.updateCount}');
    expect(src).toContain('stats.refreshUpdateCount(newId)');
    expect(functionBody('const debouncedModSetStats = debounceTrailing(')).toContain(
      'refreshUpdateCount',
    );
  });
});
