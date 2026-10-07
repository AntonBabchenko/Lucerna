// `class:text-*` on a `.btn-icon` element does nothing. `.btn-icon` sets its colour in
// src/app.css after `@tailwind utilities` and outside any layer, so at equal specificity
// it beats every `text-*` utility: the skin editor's chosen tool and the screenshot
// annotator's Thin / Thick showed no choice at all. DESIGN.md §5 already names the
// spelling that works — the important modifier, `!text-accent` — and this guard keeps the
// dead one out.
//
// Walker after tests/no-idle-live-region-box.test.ts. The class attribute is read as
// source text, so a template-literal class (`class={`btn-icon ${x}`}`) is caught too.

import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parse } from 'svelte/compiler';
import { describe, expect, it } from 'vitest';

type Attribute = { type: string; name: string; start: number; end: number };
type Node = { type: string; name?: string; attributes?: Attribute[]; [key: string]: unknown };

function svelteFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) svelteFiles(full, acc);
    else if (entry.name.endsWith('.svelte')) acc.push(full);
  }
  return acc;
}

const CHILD_KEYS = [
  'fragment',
  'consequent',
  'alternate',
  'body',
  'fallback',
  'pending',
  'then',
  'catch',
];

function walk(nodes: Node[], visit: (n: Node) => void): void {
  for (const n of nodes) {
    visit(n);
    for (const key of CHILD_KEYS) {
      const f = n[key] as { nodes?: Node[] } | null | undefined;
      if (f?.nodes) walk(f.nodes, visit);
    }
  }
}

const BTN_ICON = /(?<![\w-])btn-icon(?![\w-])/;

/** `class:text-*` directives on elements whose class names `btn-icon`. */
function noopTextDirectives(source: string): string[] {
  const ast = parse(source, { modern: true }) as unknown as { fragment: { nodes: Node[] } };
  const hits: string[] = [];
  walk(ast.fragment.nodes, (n) => {
    const attrs = n.attributes ?? [];
    const cls = attrs.find((a) => a.type === 'Attribute' && a.name === 'class');
    if (!cls || !BTN_ICON.test(source.slice(cls.start, cls.end))) return;
    for (const a of attrs) {
      if (a.type === 'ClassDirective' && a.name.startsWith('text-')) {
        hits.push(`<${n.name} class:${a.name}>`);
      }
    }
  });
  return hits;
}

describe('no class:text-* on a .btn-icon (DESIGN.md §5)', () => {
  // A guard that cannot fail is not a guard: prove the scanner discriminates
  // before trusting an empty offender list.
  it('flags the dead spelling and spares the working one', () => {
    expect(
      noopTextDirectives('<button class="btn-icon btn-icon-sm" class:text-accent={on}></button>'),
    ).toEqual(['<button class:text-accent>']);
    expect(
      // biome-ignore lint/suspicious/noTemplateCurlyInString: Svelte source holding a template-literal class
      noopTextDirectives('<button class={`btn-icon ${x}`} class:text-muted={off}></button>'),
    ).toEqual(['<button class:text-muted>']);
    expect(
      noopTextDirectives(
        '{#if a}{#each xs as x}<button class="btn-icon" class:text-accent={x}></button>{/each}{/if}',
      ),
    ).toEqual(['<button class:text-accent>']);
    expect(
      noopTextDirectives(`<button class="btn-icon {on ? '!text-accent' : ''}"></button>`),
    ).toEqual([]);
    expect(noopTextDirectives('<a class="tab" class:text-primary={on}>x</a>')).toEqual([]);
    expect(noopTextDirectives('<span class="btn-icon-sm" class:text-accent={on}></span>')).toEqual(
      [],
    );
  });

  it('src has none', () => {
    const offenders = svelteFiles('src').flatMap((f) =>
      noopTextDirectives(readFileSync(f, 'utf8')).map((h) => `${f}: ${h}`),
    );
    expect(offenders).toEqual([]);
  });
});
