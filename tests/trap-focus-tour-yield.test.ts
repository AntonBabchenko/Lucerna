// `trapFocus` yields initial focus while a contextual tour lies above its
// dialog in the layer stack (src/lib/ui/layer-stack.svelte.ts), so a tour
// hosted by the dialog keeps focus on its card. It yields to nothing else: a
// page tour BELOW the dialog is not on screen and must not disable the trap.
// This file also pins the other half: the yield has to be given back.
//
// Without it the dialog is left with no focus at all — the tour ends by
// unmounting, which drops focus to <body>, and nothing would ever move it into
// the panel. A screen reader never announces the dialog, and the user's place
// in it is lost: a Tab from <body> starts again at the dialog's first control
// (before trapFocus took a Tab pressed with the focus on <body>, it walked the
// application behind the open dialog).
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

// 2026-10-02 regression F11: the give-back was armed only for a tour already on top when the
// dialog took its initial focus, and not even then once the tour's card — a child of the panel —
// held the focus by that time. A tour that arrives LATER (the dialog's content loaded first, or the
// tour was started from inside it) moved the focus to its card and, ending, dropped it to <body>:
// Tab then walked the page behind the open dialog (translations editor, Manage).
describe('trapFocus gives focus back after a tour that arrived later', () => {
  // The dialog is open with focus on a field the user was using; then its tour arrives and
  // moves focus to the card, which is a child of the panel.
  async function tourArrivesOverFocusedField(): Promise<{
    target: HTMLButtonElement;
    field: HTMLInputElement;
    card: HTMLButtonElement;
    endTour: () => void;
  }> {
    const dialog = newLayerId('dialog');
    pushLayer(dialog, 'modal', noop);
    const { panel, target } = mountPanel(dialog);
    await frame();
    expect(document.activeElement).toBe(target);
    const field = document.createElement('input');
    panel.appendChild(field);
    field.focus();

    const endTour = insertTour(newLayerId('tour'), dialog, noop);
    if (!endTour) throw new Error('tour refused');
    const card = document.createElement('button');
    card.textContent = 'Next';
    panel.appendChild(card);
    card.focus();
    return { target, field, card, endTour };
  }

  it('puts focus back where it was in the dialog when the tour ends', async () => {
    const { field, card, endTour } = await tourArrivesOverFocusedField();
    card.remove();
    endTour();
    expect(document.activeElement).toBe(field);
  });

  it('also when the card that held the focus outlives the tour by an update', async () => {
    const { field, card, endTour } = await tourArrivesOverFocusedField();
    endTour(); // the layer goes first; the card is unmounted in the same update
    card.remove();
    await Promise.resolve();
    expect(document.activeElement).toBe(field);
  });

  it('falls back to the initial target when that element is gone', async () => {
    const { target, field, card, endTour } = await tourArrivesOverFocusedField();
    field.remove();
    card.remove();
    endTour();
    expect(document.activeElement).toBe(target);
  });

  it('a tour stepping aside under a popover is not an end', async () => {
    const { card, endTour } = await tourArrivesOverFocusedField();
    // A popover opened over the tour takes the top; the tour is still the dialog's.
    const releasePopover = pushLayer(newLayerId('popover'), 'popover', noop);
    expect(document.activeElement).toBe(card);
    releasePopover();
    expect(document.activeElement).toBe(card);
    endTour();
  });
});
