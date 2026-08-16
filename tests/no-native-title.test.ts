// The design system bans native `title=` — docs/DESIGN.md §5:
//   "Never `title=` — it is unreliable for assistive tech / touch and
//    bypasses the singleton tooltip layer. (A `title` prop forwarded *into*
//    `use:tooltip` internally is fine — the prop name is incidental.)"
//
// tests/l10n-key-table.test.ts already asserts the absence on ONE surface
// (the AI-origin badge and the truncated key). That is a local regression
// lock, not the ban: nothing stopped the next component from adding one, and
// five did. This file is the ban, shaped after tests/no-native-select.test.ts
// — the same "scan src for the forbidden construct" guard, and the same
// `svelteFiles` walker.
//
// The scan has to tell two spellings apart, because §5 allows one of them:
//   BANNED   <button title={reason}>        — a native element attribute
//   ALLOWED  <StatusBadge title={reason} /> — a component prop, forwarded
//                                             into use:tooltip internally
// Svelte itself draws that line by case: a lowercase tag name is an HTML
// element, a capitalised one is a component.
//
// Known limit: an attribute that reaches a native element through a `{...rest}`
// spread is invisible to a source scan. That is why the components declaring a
// `title` prop (BusyButton, StatusBadge, HelpPopover, CountPill) destructure it
// out of `rest` and hand it to `use:tooltip` rather than letting it fall
// through onto the DOM.

import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

function svelteFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) svelteFiles(full, acc);
    else if (entry.name.endsWith('.svelte')) acc.push(full);
  }
  return acc;
}

/**
 * `source` with every `{ … }` expression span removed (nesting-aware). A Svelte
 * attribute value is JavaScript and routinely contains a `>` —
 * `onclick={() => next()}` is the common one — so a raw search for the end of
 * an opening tag would stop inside an expression and misread every attribute
 * after it as page text.
 */
function withoutExpressions(source: string): string {
  let out = '';
  let depth = 0;
  for (const ch of source) {
    if (ch === '{') depth += 1;
    else if (ch === '}') depth = Math.max(0, depth - 1);
    else if (depth === 0) out += ch;
  }
  return out;
}

/** Tag names of `title=` / `{title}` occurrences that sit inside the opening
 *  tag of a lowercase-named (i.e. native HTML) element. */
function nativeTitleAttributes(source: string): string[] {
  const hits: string[] = [];
  const candidate = /(?:^|\s)(title\s*=|\{title\})/g;
  let m: RegExpExecArray | null = candidate.exec(source);
  while (m !== null) {
    const at = m.index + m[0].length - m[1].length;
    const open = source.lastIndexOf('<', at);
    const tag = open === -1 ? null : /^<([a-zA-Z][\w.:-]*)/.exec(source.slice(open, at));
    // Still inside that opening tag? If the text between the `<` and the match
    // closes the tag, the match is page content (`<p>{title}</p>`), not an
    // attribute.
    const closed = tag !== null && withoutExpressions(source.slice(open, at)).includes('>');
    if (tag !== null && !closed && tag[1][0] === tag[1][0].toLowerCase()) hits.push(tag[1]);
    m = candidate.exec(source);
  }
  return hits;
}

describe('no native title= in src (docs/DESIGN.md §5)', () => {
  // A guard that cannot fail is not a guard: prove the scanner discriminates
  // before trusting an empty offender list.
  it('flags a native element and spares a component', () => {
    expect(nativeTitleAttributes('<button title={reason}>x</button>')).toEqual(['button']);
    expect(nativeTitleAttributes('<span class="a" title={n}>x</span>')).toEqual(['span']);
    expect(nativeTitleAttributes('<button {title}>x</button>')).toEqual(['button']);
    expect(nativeTitleAttributes('<StatusBadge title={n} />')).toEqual([]);
    expect(nativeTitleAttributes('<p class="x">{title}</p>')).toEqual([]);
    expect(nativeTitleAttributes('<b use:tooltip={title}>x</b>')).toEqual([]);
    expect(nativeTitleAttributes('<button onclick={() => f(a > b)} title={n}>x</button>')).toEqual([
      'button',
    ]);
  });

  it('every hover / disabled-reason string goes through use:tooltip', () => {
    const offenders = svelteFiles('src')
      .map((file) => ({ file, tags: nativeTitleAttributes(readFileSync(file, 'utf8')) }))
      .filter((entry) => entry.tags.length > 0)
      .map((entry) => `${entry.file}: <${entry.tags.join('>, <')}>`);
    expect(offenders).toEqual([]);
  });
});
