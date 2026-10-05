// A Tab pressed while nothing holds the focus — `<body>`: a toast above the dialog that removed
// itself, a focused control that was removed — never reaches the panel's own Tab handler, since a
// key event goes to the focused element. trapFocus takes it at the document for the topmost modal
// only, and not while a tour lies above that modal. It never takes a focus something holds.
// Real focus moves are in tests-e2e/modal-scrim-focus.spec.ts; happy-dom has no layout, so here
// the trap may fall back to the panel itself — "the focus is in the panel" is what is asserted.
import { afterEach, describe, expect, it } from 'vitest';

import {
  __resetLayers,
  insertTour,
  type LayerId,
  newLayerId,
  pushLayer,
} from '$lib/ui/layer-stack.svelte';
import { trapFocus } from '$lib/ui/trap-focus';

const cleanups: Array<() => void> = [];

afterEach(() => {
  for (const fn of cleanups.splice(0).reverse()) fn();
  __resetLayers();
  document.body.innerHTML = '';
});

const noop = () => {};

function openDialog(): { layer: LayerId; panel: HTMLElement; destroyTrap: () => void } {
  const layer = newLayerId('dialog');
  cleanups.push(pushLayer(layer, 'modal', noop));
  const panel = document.createElement('div');
  panel.tabIndex = -1;
  const button = document.createElement('button');
  button.textContent = 'OK';
  panel.appendChild(button);
  document.body.appendChild(panel);
  const handle = trapFocus(panel, layer);
  let destroyed = false;
  const destroyTrap = () => {
    if (destroyed) return;
    destroyed = true;
    handle.destroy();
  };
  cleanups.push(destroyTrap);
  return { layer, panel, destroyTrap };
}

/** A key press as the browser sends it: to the focused element, `<body>` when none. */
function press(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  (document.activeElement ?? document.body).dispatchEvent(e);
  return e;
}

const pressTab = (init: KeyboardEventInit = {}) => press('Tab', init);

function dropFocus(): void {
  (document.activeElement as HTMLElement | null)?.blur();
  expect(document.activeElement).toBe(document.body);
}

describe('trapFocus and a Tab pressed while nothing holds the focus', () => {
  it('takes it into the top dialog', () => {
    const { panel } = openDialog();
    dropFocus();
    const e = pressTab();
    expect(e.defaultPrevented).toBe(true);
    expect(panel.contains(document.activeElement)).toBe(true);
  });

  it('Shift+Tab too', () => {
    const { panel } = openDialog();
    dropFocus();
    const e = pressTab({ shiftKey: true });
    expect(e.defaultPrevented).toBe(true);
    expect(panel.contains(document.activeElement)).toBe(true);
  });

  it('only the topmost dialog takes it', () => {
    const lower = openDialog();
    const upper = openDialog();
    dropFocus();
    pressTab();
    expect(upper.panel.contains(document.activeElement)).toBe(true);
    expect(lower.panel.contains(document.activeElement)).toBe(false);
  });

  it('a popover above the dialog does not stop it', () => {
    const { panel } = openDialog();
    cleanups.push(pushLayer(newLayerId('popover'), 'popover', noop));
    dropFocus();
    pressTab();
    expect(panel.contains(document.activeElement)).toBe(true);
  });

  it('leaves it to a tour lying above the dialog', () => {
    const { layer } = openDialog();
    const endTour = insertTour(newLayerId('tour'), layer, noop);
    if (!endTour) throw new Error('tour refused');
    cleanups.push(endTour);
    dropFocus();
    const e = pressTab();
    expect(e.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(document.body);
  });

  it('never takes a focus something outside the dialog holds', () => {
    openDialog();
    const outside = document.createElement('button');
    document.body.appendChild(outside);
    outside.focus();
    const e = pressTab();
    expect(e.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(outside);
  });

  it('ignores other keys', () => {
    openDialog();
    dropFocus();
    const e = press('Enter');
    expect(e.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(document.body);
  });

  it('leaves a Tab that a handler before it already took', () => {
    openDialog();
    dropFocus();
    const take = (e: Event) => e.preventDefault();
    document.body.addEventListener('keydown', take);
    cleanups.push(() => document.body.removeEventListener('keydown', take));
    pressTab();
    expect(document.activeElement).toBe(document.body);
  });

  it('stops listening once the trap is gone, though its layer is still on top', () => {
    const { panel, destroyTrap } = openDialog();
    destroyTrap();
    dropFocus();
    const e = pressTab();
    expect(e.defaultPrevented).toBe(false);
    expect(panel.contains(document.activeElement)).toBe(false);
  });
});
