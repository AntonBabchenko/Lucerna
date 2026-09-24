// An idle status line must not take a layout slot.
//
// A live region has to stay mounted while it is empty, so that a later message
// is announced. But an empty in-flow box is still a flex/grid item, and a
// container puts its gap on both sides of it: the Settings → Appearance
// sidebar-button list showed a double gap between "Add account" and "Manage",
// and 33 other places did the same. StatusMessage now keeps its idle region
// out of flow (`absolute`, still in the accessibility tree) and takes its test
// hook itself (`dataTestid`), so nothing has to wrap it. This file keeps both
// halves true:
//
//   rule A  an element whose only child is a <StatusMessage> — the wrapper is
//           the same empty box one level up. Pass `dataTestid` / `class` /
//           `bind:element` to StatusMessage instead.
//   rule B  a hand-built persistent live region (role status/alert/log, a role
//           expression naming alert or status, or aria-live) whose content is
//           only conditional, and which stays in flow. It is exempt when it is
//           out of flow or deliberately reserves its line: a static class with
//           sr-only / absolute / fixed / min-h-*, a `class:absolute` directive,
//           or a class expression containing 'absolute'. StatusMessage.svelte
//           itself is held to this rule.
//
// Known limit: a StatusMessage passed as children into a component that wraps
// it is invisible to a source scan (none today). tests-e2e/phantom-gap.spec.ts
// measures the real layout of the Settings pages and covers that case there.

import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { parse } from 'svelte/compiler';
import { describe, expect, it } from 'vitest';

type TextPart = { type: string; data?: string; start: number; end: number };
type Attribute = {
  type: string;
  name: string;
  value: true | TextPart | TextPart[];
};
type Node = {
  type: string;
  name?: string;
  data?: string;
  start: number;
  attributes?: Attribute[];
  [key: string]: unknown;
};

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

function childrenOf(node: Node): Node[] {
  const out: Node[] = [];
  for (const key of CHILD_KEYS) {
    const f = node[key] as { nodes?: Node[] } | null | undefined;
    if (f?.nodes) out.push(...f.nodes);
  }
  return out;
}

function walk(nodes: Node[], visit: (n: Node) => void): void {
  for (const n of nodes) {
    visit(n);
    walk(childrenOf(n), visit);
  }
}

const meaningful = (nodes: Node[]) =>
  nodes.filter(
    (n) => !(n.type === 'Comment' || (n.type === 'Text' && (n.data ?? '').trim() === '')),
  );

function attr(el: Node, name: string): Attribute | undefined {
  return el.attributes?.find((a) => a.type === 'Attribute' && a.name === name);
}

/** The literal text of an attribute (static parts only). */
function staticText(a: Attribute | undefined): string {
  if (!a || a.value === true) return '';
  const parts = Array.isArray(a.value) ? a.value : [a.value];
  return parts
    .filter((p) => p.type === 'Text')
    .map((p) => p.data ?? '')
    .join(' ');
}

/** The source of every `{ … }` part of an attribute value. */
function expressionSource(a: Attribute | undefined, src: string): string {
  if (!a || a.value === true) return '';
  const parts = Array.isArray(a.value) ? a.value : [a.value];
  return parts
    .filter((p) => p.type === 'ExpressionTag')
    .map((p) => src.slice(p.start, p.end))
    .join(' ');
}

const LIVE_ROLE = /^(status|alert|log)$/;
const OUT_OF_FLOW_CLASS = /(^|\s)(sr-only|absolute|fixed|min-h-\S+)(\s|$)/;

/** Offenders in one component's source, as `rule file:line`. */
function offendersIn(src: string, file: string): string[] {
  const ast = parse(src, { modern: true }) as unknown as { fragment: { nodes: Node[] } };
  const line = (i: number) => src.slice(0, i).split('\n').length;
  const out: string[] = [];
  walk(ast.fragment.nodes, (n) => {
    if (n.type !== 'RegularElement') return;
    const kids = meaningful(childrenOf(n));

    if (kids.length === 1 && kids[0].type === 'Component' && kids[0].name === 'StatusMessage') {
      out.push(`A ${file}:${line(n.start)}`);
    }

    const role = attr(n, 'role');
    const roleIsLive =
      LIVE_ROLE.test(staticText(role).trim()) ||
      /['"](alert|status)['"]/.test(expressionSource(role, src));
    const isLive = roleIsLive || attr(n, 'aria-live') !== undefined;
    if (!isLive || kids.length === 0 || !kids.every((k) => k.type === 'IfBlock')) return;
    const cls = attr(n, 'class');
    const exempt =
      OUT_OF_FLOW_CLASS.test(staticText(cls)) ||
      expressionSource(cls, src).includes('absolute') ||
      (n.attributes ?? []).some((a) => a.type === 'ClassDirective' && a.name === 'absolute');
    if (!exempt) out.push(`B ${file}:${line(n.start)}`);
  });
  return out;
}

describe('an idle status line takes no layout slot', () => {
  it('the scan can fail: it flags the shapes it bans and spares the ones it allows', () => {
    const check = (src: string) => offendersIn(src, 'fixture.svelte');
    // Rule A — the exact shape of the reported bug, and the testid wrapper.
    expect(check('<div class="pl-6"><StatusMessage message={m} tone="info" /></div>')).toEqual([
      'A fixture.svelte:1',
    ]);
    expect(check('<div data-testid="x">\n  <StatusMessage message={m} />\n</div>')).toEqual([
      'A fixture.svelte:1',
    ]);
    expect(check('<StatusMessage dataTestid="x" message={m} />')).toEqual([]);
    expect(check('<div><StatusMessage message={m} /><button>Retry</button></div>')).toEqual([]);
    // Rule B — a hand-built region, static role or a role expression.
    expect(check('<div role="status">{#if a}<p>{a}</p>{/if}</div>')).toEqual([
      'B fixture.svelte:1',
    ]);
    expect(check("<div role={x ? 'alert' : 'status'}>{#if a}<p>{a}</p>{/if}</div>")).toEqual([
      'B fixture.svelte:1',
    ]);
    expect(check('<div aria-live="polite">{#if a}<p>{a}</p>{/if}</div>')).toEqual([
      'B fixture.svelte:1',
    ]);
    expect(check('<div role="status" class="sr-only">{#if a}<p>{a}</p>{/if}</div>')).toEqual([]);
    expect(check('<div role="status" class="min-h-4">{#if a}<p>{a}</p>{/if}</div>')).toEqual([]);
    expect(check('<div role="status" class:absolute={!a}>{#if a}<p>{a}</p>{/if}</div>')).toEqual(
      [],
    );
    expect(
      check(
        "<div role={x ? 'alert' : 'status'} class={a ? undefined : 'absolute'}>{#if a}<p>{a}</p>{/if}</div>",
      ),
    ).toEqual([]);
    // A region with real content next to its conditional part is not idle-empty.
    expect(check('<div role="status"><span>Label</span>{#if a}<p>{a}</p>{/if}</div>')).toEqual([]);
  });

  it('no component wraps a StatusMessage or leaves an idle live region in flow', () => {
    const root = join(import.meta.dirname, '..');
    const offenders = svelteFiles(join(root, 'src')).flatMap((f) =>
      offendersIn(readFileSync(f, 'utf8'), relative(root, f).replaceAll('\\', '/')),
    );
    expect(offenders).toEqual([]);
  });
});
