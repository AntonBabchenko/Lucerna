// The datapack row's expander is a MANUAL toggle (a `<button aria-expanded>`
// over a SvelteSet), not a `<details>`, so registry.ts's second reveal
// mechanism applies: swap the icon name between `caret` (collapsed) and
// `chevronDown` (expanded). It was using `chevronRight` — the PAGINATION name.
//
// This contract can only be pinned at the source. `caret` and `chevronRight`
// are the SAME Lucide component today (registry.ts: `caret: ChevronRight` and
// `chevronRight: ChevronRight`), so no DOM assertion can tell the two apart —
// a rendered-glyph test would pass identically before and after the fix and
// would be worth nothing. Same reasoning as tests/intent/design-tokens.test.ts,
// which asserts on src/app.css directly "the way tests/intent/browser-feel.test.ts
// asserts the CSS rules happy-dom cannot compute".
//
// Pinning the whole expression (rather than just the absence of the wrong name)
// is what stops the fix being "satisfied" by deleting the swap altogether:
// without a <details> there is no `.disclosure-caret` CSS rule to hang the
// rotation off, so the name swap IS the affordance here.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const source = readFileSync(
  resolve(process.cwd(), 'src/lib/mods/InstalledDatapacksView.svelte'),
  'utf8',
);

/** `source` with HTML and block comments removed. The markup is heavily
 *  commented with exactly the rationale these assertions are about — including
 *  the words `<details>` and `.disclosure-caret` — so a raw substring search
 *  would be answered by the prose rather than by the code. */
function withoutComments(text: string): string {
  return text.replace(/<!--[\s\S]*?-->/g, '').replace(/\/\*[\s\S]*?\*\//g, '');
}

describe('InstalledDatapacksView row expander (docs/DESIGN.md §7)', () => {
  it('names the collapsed disclosure glyph `caret`, not the pagination `chevronRight`', () => {
    // §7: "Names are intent-based, not glyph-based. Notable pairs: `caret`
    // (collapsed) vs `chevronDown` (expanded)".
    expect(source).toContain("name={isOpen ? 'chevronDown' : 'caret'}");
    expect(withoutComments(source)).not.toContain("'chevronRight'");
  });

  it('is a manual toggle, so the swap is the affordance and must survive', () => {
    // `details[open] > summary .disclosure-caret` is the OTHER mechanism
    // registry.ts sanctions, and its selector would never match here — there is
    // no <details> in this file. Deleting the swap in the name of "use
    // .disclosure-caret everywhere" would leave the row with a glyph that never
    // reflects its state.
    const code = withoutComments(source);
    expect(code).not.toContain('<details');
    expect(code).not.toContain('disclosure-caret');
    expect(code).toContain('aria-expanded={isOpen}');
  });
});
