// `trapFocus` yields initial focus while a contextual onboarding tour owns the
// screen, so opening a dialog behind a tour does not yank focus off the tour's
// popover. This file pins the OTHER half: the yield has to be given back.
//
// Without it the dialog is left with no focus at all — the tour ends by
// unmounting, which drops focus to <body>, and nothing would ever move it into
// the panel. A screen reader never announces the dialog, and because the Tab
// handler is registered on the panel node, a keydown targeted at <body> never
// reaches it: Tab walks the application *behind* the open dialog instead of
// cycling inside it.
//
// Reachable since the `overview` contextual tour began auto-firing at startup
// on the default tab, at the same moment the post-update changelog offer
// appears; the toast's action button removes itself on click, so focus is
// already on <body> by the time the dialog's deferred initial focus runs.
import { afterEach, describe, expect, it } from 'vitest';

import { trapFocus } from '$lib/ui/trap-focus';

const ATTR = 'data-ctx-tour-active';

let cleanup: (() => void) | null = null;

afterEach(() => {
  cleanup?.();
  cleanup = null;
  document.body.removeAttribute(ATTR);
  document.body.innerHTML = '';
});

/** A panel with an explicit autofocus target — `[data-autofocus]` is found by a
 *  plain querySelector, unlike the focusable sweep, which filters on
 *  `offsetParent` and so returns nothing in a layout-less test DOM. */
function mountPanel(): { panel: HTMLElement; target: HTMLButtonElement } {
  const panel = document.createElement('div');
  panel.tabIndex = -1;
  const target = document.createElement('button');
  target.setAttribute('data-autofocus', '');
  target.textContent = 'Close';
  panel.appendChild(target);
  document.body.appendChild(panel);
  const handle = trapFocus(panel);
  cleanup = () => handle.destroy();
  return { panel, target };
}

const frame = (): Promise<void> => new Promise((r) => requestAnimationFrame(() => r()));
const microtask = (): Promise<void> => new Promise((r) => queueMicrotask(() => r()));

describe('trapFocus yields to a contextual tour, then takes focus back', () => {
  it('does not pull focus while the tour owns the screen', async () => {
    document.body.setAttribute(ATTR, 'true');
    const { target } = mountPanel();
    await frame();
    expect(document.activeElement).not.toBe(target);
  });

  it('takes initial focus once the tour releases the screen', async () => {
    document.body.setAttribute(ATTR, 'true');
    const { target } = mountPanel();
    await frame();
    expect(document.activeElement).not.toBe(target);

    document.body.removeAttribute(ATTR);
    await microtask();
    expect(document.activeElement).toBe(target);
  });

  it('leaves focus alone if the user already clicked into the panel', async () => {
    document.body.setAttribute(ATTR, 'true');
    const { panel, target } = mountPanel();
    await frame();

    const other = document.createElement('input');
    panel.appendChild(other);
    other.focus();

    document.body.removeAttribute(ATTR);
    await microtask();
    expect(document.activeElement).toBe(other);
    expect(document.activeElement).not.toBe(target);
  });

  it('takes focus immediately when no tour is up, unchanged from before', async () => {
    const { target } = mountPanel();
    await frame();
    expect(document.activeElement).toBe(target);
  });
});
