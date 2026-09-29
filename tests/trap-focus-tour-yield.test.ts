// `trapFocus` yields initial focus while a contextual tour lies above its
// dialog in the layer stack (src/lib/ui/layer-stack.svelte.ts), so a tour
// hosted by the dialog keeps focus on its card. It yields to nothing else: a
// page tour BELOW the dialog is not on screen and must not disable the trap.
// This file also pins the other half: the yield has to be given back.
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

import {
  __resetLayers,
  insertTour,
  type LayerId,
  newLayerId,
  pushLayer,
} from '$lib/ui/layer-stack.svelte';
import { trapFocus } from '$lib/ui/trap-focus';

let cleanup: (() => void) | null = null;

afterEach(() => {
  cleanup?.();
  cleanup = null;
  __resetLayers();
  document.body.innerHTML = '';
});

function mountPanel(layer: LayerId): { panel: HTMLElement; target: HTMLButtonElement } {
  const panel = document.createElement('div');
  panel.tabIndex = -1;
  const target = document.createElement('button');
  target.setAttribute('data-autofocus', '');
  target.textContent = 'Close';
  panel.appendChild(target);
  document.body.appendChild(panel);
  const handle = trapFocus(panel, layer);
  cleanup = () => handle.destroy();
  return { panel, target };
}

const frame = (): Promise<void> => new Promise((r) => requestAnimationFrame(() => r()));
const noop = () => {};

function dialogWithTourAbove(): { dialog: LayerId; endTour: () => void } {
  const dialog = newLayerId('dialog');
  pushLayer(dialog, 'modal', noop);
  const endTour = insertTour(newLayerId('tour'), dialog, noop);
  if (!endTour) throw new Error('tour refused');
  return { dialog, endTour };
}

describe('trapFocus yields to a tour above its dialog, then takes focus back', () => {
  it('does not pull focus while the tour is on top', async () => {
    const { dialog } = dialogWithTourAbove();
    const { target } = mountPanel(dialog);
    await frame();
    expect(document.activeElement).not.toBe(target);
  });

  it('takes initial focus once the tour ends', async () => {
    const { dialog, endTour } = dialogWithTourAbove();
    const { target } = mountPanel(dialog);
    await frame();
    endTour();
    expect(document.activeElement).toBe(target);
  });

  it('leaves focus alone if the user already clicked into the panel', async () => {
    const { dialog, endTour } = dialogWithTourAbove();
    const { panel, target } = mountPanel(dialog);
    await frame();
    const other = document.createElement('input');
    panel.appendChild(other);
    other.focus();
    endTour();
    expect(document.activeElement).toBe(other);
    expect(document.activeElement).not.toBe(target);
  });

  it('takes focus immediately when no tour is up', async () => {
    const dialog = newLayerId('dialog');
    pushLayer(dialog, 'modal', noop);
    const { target } = mountPanel(dialog);
    await frame();
    expect(document.activeElement).toBe(target);
  });

  it('a page tour BELOW the dialog does not hold its focus back', async () => {
    const dialog = newLayerId('dialog');
    pushLayer(dialog, 'modal', noop);
    insertTour(newLayerId('page-tour'), null, noop);
    const { target } = mountPanel(dialog);
    await frame();
    expect(document.activeElement).toBe(target);
  });
});
