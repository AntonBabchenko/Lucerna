// Cluster H: browser-feel cleanup. Asserts that the three browser-native
// behaviours we suppress in app.css + +layout.svelte stay suppressed:
//   1. Global `contextmenu` is preventDefault'd at the window level.
//   2. CSS user-select base reset is present in src/app.css.
//   3. The .selectable opt-in utility is defined.
//
// happy-dom does not implement getComputedStyle for `user-select` /
// `-webkit-user-drag` in a way that survives layered Tailwind builds, so
// the visual effect is asserted by reading src/app.css directly and
// confirming the rules are there. The contextmenu suppression IS testable
// at runtime — we mount the layout, dispatch a contextmenu event on the
// window, and check defaultPrevented.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import Layout from '../../src/routes/+layout.svelte';

const APP_CSS_PATH = resolve(process.cwd(), 'src/app.css');
const appCss = readFileSync(APP_CSS_PATH, 'utf8');

describe('Cluster H — contextmenu suppression', () => {
  it('+layout.svelte preventDefaults right-click on a non-editable target', () => {
    // The layout takes a `children` Snippet prop. The intent test only
    // cares about the <svelte:window oncontextmenu> registration, not the
    // body, so pass an empty render-nothing snippet via a type cast.
    const emptySnippet = (() => null) as unknown as never;
    render(Layout, { props: { children: emptySnippet } });
    const evt = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    window.dispatchEvent(evt);
    expect(evt.defaultPrevented).toBe(true);
  });

  it('preserves native right-click menu inside an <input> (M1 fix)', () => {
    // Paste-heavy editable fields (CurseForge API key, name entry) keep
    // their native Cut/Copy/Paste menu so right-click → Paste still works
    // without a custom dropdown.
    const emptySnippet = (() => null) as unknown as never;
    render(Layout, { props: { children: emptySnippet } });
    const input = document.createElement('input');
    document.body.appendChild(input);
    const evt = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    input.dispatchEvent(evt);
    expect(evt.defaultPrevented).toBe(false);
    document.body.removeChild(input);
  });

  it('preserves native right-click menu inside a <textarea> (M1 fix)', () => {
    const emptySnippet = (() => null) as unknown as never;
    render(Layout, { props: { children: emptySnippet } });
    const textarea = document.createElement('textarea');
    document.body.appendChild(textarea);
    const evt = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    textarea.dispatchEvent(evt);
    expect(evt.defaultPrevented).toBe(false);
    document.body.removeChild(textarea);
  });

  it('preserves native right-click menu inside a contenteditable=true element (M1 fix)', () => {
    const emptySnippet = (() => null) as unknown as never;
    render(Layout, { props: { children: emptySnippet } });
    const div = document.createElement('div');
    div.setAttribute('contenteditable', 'true');
    document.body.appendChild(div);
    const evt = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    div.dispatchEvent(evt);
    expect(evt.defaultPrevented).toBe(false);
    document.body.removeChild(div);
  });
});

describe('Cluster H — app.css user-select reset', () => {
  it('the @layer base html/body block declares user-select: none', () => {
    // app.css has TWO `html, body` selectors — a plain one early in the
    // file (just height + margin) and the styled one inside @layer base.
    // Match the latter by scoping inside the @layer base { … } body.
    const layerBase = appCss.match(/@layer\s+base\s*\{[\s\S]*?\n\}\s*\n/);
    expect(layerBase, '@layer base block must exist').not.toBeNull();
    const bodyBlock = layerBase?.[0].match(/html,\s*\n?\s*body\s*\{[\s\S]*?\n\s*\}/);
    expect(bodyBlock, 'html, body { ... } inside @layer base').not.toBeNull();
    expect(bodyBlock?.[0]).toMatch(/user-select:\s*none/);
  });

  it('input / textarea / contenteditable opt back into user-select: text', () => {
    const inputBlock = appCss.match(
      /input,\s*\n?\s*textarea,\s*\n?\s*\[contenteditable='true'\]\s*\{[\s\S]*?\}/,
    );
    expect(inputBlock).not.toBeNull();
    expect(inputBlock?.[0]).toMatch(/user-select:\s*text/);
  });

  it('img has -webkit-user-drag: none', () => {
    const imgBlock = appCss.match(/img\s*\{[\s\S]*?\}/);
    expect(imgBlock).not.toBeNull();
    expect(imgBlock?.[0]).toMatch(/-webkit-user-drag:\s*none/);
  });

  it('.selectable utility class sets user-select: text', () => {
    expect(appCss).toMatch(/\.selectable\s*\{[\s\S]*?user-select:\s*text/);
  });
});
