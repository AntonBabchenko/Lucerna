/**
 * The dependency graph cache is per-instance, and `+page.svelte` is the one listener mounted all
 * session. Every mod event must drop the changed instance's graph there, or an Installed view
 * opened later seeds from a graph built before a change made elsewhere — the Play gate's repair
 * switches a dependency on, and the tree still offers to enable it. The page is the whole app
 * shell and is not renderable under vitest, so — like `tests/page-preflight-wiring.test.ts` — this
 * reads the source. The Installed view's own re-resolve is pinned by rendering tests
 * (`tests/installed-library-chips.test.ts`).
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const src = readFileSync(resolve('src/routes/+page.svelte'), 'utf8');

/** The argument list of the `.listen(` call on `events.<name>`, parenthesis-matched. */
function listener(name: string): string {
  const at = src.indexOf(`events.${name}`);
  expect(at, `+page.svelte must listen to events.${name}`).toBeGreaterThan(-1);
  const open = src.indexOf('(', src.indexOf('.listen', at));
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    if (src[i] === '(') depth++;
    else if (src[i] === ')' && --depth === 0) return src.slice(open, i + 1);
  }
  throw new Error(`unbalanced parentheses after events.${name}.listen`);
}

describe('+page drops a profile’s dependency graph on every mod event', () => {
  for (const name of ['modInstalled', 'modUninstalled', 'modToggle', 'modsReconciled']) {
    it(`on ${name}`, () => {
      expect(listener(name)).toContain('depGraphCache.delete(payload.instance_id)');
    });
  }
});
