import { fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(async () => ({ status: 'error', error: 'unused' })),
    appSettingsMarkTourCompleted: vi.fn(async () => ({ status: 'ok', data: null })),
  },
}));

import { markSeen } from '../src/lib/onboarding/contextual-tours';
import { tourState } from '../src/lib/onboarding/state.svelte';
import type { TourStep } from '../src/lib/onboarding/steps';
import { __resetLayers } from '../src/lib/ui/layer-stack.svelte';
import TourHooks from './fixtures/TourHooks.svelte';

const STEPS: TourStep[] = [
  {
    titleKey: 'onboarding.contextual.manage.yourInstances.title',
    bodyKey: 'onboarding.contextual.manage.yourInstances.body',
    targetSelector: '[data-tour-ctx="hook-a"]',
    anchor: 'below',
  },
  {
    titleKey: 'onboarding.contextual.manage.editAndSave.title',
    bodyKey: 'onboarding.contextual.manage.editAndSave.body',
    targetSelector: '[data-tour-ctx="hook-b"]',
    anchor: 'below',
    reveal: true,
  },
];

const settle = async () => {
  await tick();
  await tick();
  await new Promise<void>((r) => queueMicrotask(r));
};
const primary = (): HTMLElement => {
  const el = document.querySelector<HTMLElement>('[data-tour-primary]');
  if (!el) throw new Error('no tour primary button on screen');
  return el;
};
const back = () => screen.getByRole('button', { name: /back/i });

describe('ContextualTour onStep', () => {
  beforeEach(() => {
    localStorage.clear();
    __resetLayers();
    tourState.active = false;
    tourState.contextual = false;
    tourState.currentStep = 0;
  });

  it('reports each step as it shows: 0 on start, 1 on Next, 0 on Back', async () => {
    const spy = vi.fn();
    render(TourHooks, { props: { steps: STEPS, spy } });
    await settle();
    expect(spy.mock.calls.map((c) => c[0])).toEqual([0]);
    await fireEvent.click(primary());
    await settle();
    await fireEvent.click(back());
    await settle();
    expect(spy.mock.calls.map((c) => c[0])).toEqual([0, 1, 0]);
  });

  it('is never called for a tour already seen', async () => {
    markSeen('manage');
    const spy = vi.fn();
    render(TourHooks, { props: { steps: STEPS, spy } });
    await settle();
    expect(spy).not.toHaveBeenCalled();
  });

  // The host's own state read inside onStep must not become the tour's dependency: the user
  // collapsing what the step opened would re-run the step, re-open it and pull focus to the card.
  it('does not re-run when the host state onStep read changes', async () => {
    const spy = vi.fn();
    render(TourHooks, { props: { steps: STEPS, spy } });
    await settle();
    await fireEvent.click(primary());
    await settle();
    expect(document.querySelector('[data-tour-ctx="hook-b"]')).not.toBeNull();
    const calls = spy.mock.calls.length;
    await fireEvent.click(screen.getByRole('button', { name: 'Toggle B' }));
    await settle();
    expect(spy.mock.calls.length).toBe(calls);
    expect(document.querySelector('[data-tour-ctx="hook-b"]')).toBeNull();
  });
});

describe('ContextualTour reveal', () => {
  const original = Element.prototype.scrollIntoView;
  const scroll = vi.fn();
  beforeEach(() => {
    localStorage.clear();
    __resetLayers();
    tourState.active = false;
    scroll.mockClear();
    Element.prototype.scrollIntoView = scroll;
  });
  afterEach(() => {
    Element.prototype.scrollIntoView = original;
  });

  it("scrolls a reveal step's anchor into view once it exists, and leaves other steps alone", async () => {
    render(TourHooks, { props: { steps: STEPS, spy: () => {} } });
    await settle();
    expect(scroll).not.toHaveBeenCalled();
    await fireEvent.click(primary());
    await settle();
    expect(scroll).toHaveBeenCalledWith({ block: 'nearest', inline: 'nearest' });
    expect((scroll.mock.contexts.at(-1) as Element).getAttribute('data-tour-ctx')).toBe('hook-b');
  });
});
