import { beforeEach, describe, expect, it, vi } from 'vitest';

const { windowSetCompact, appSettingsGet, appSettingsSetGeneral } = vi.hoisted(() => {
  const sampleGeneral = {
    hide_to_tray_during_game: false,
    theme: 'system',
    check_updates_on_startup: true,
    language: 'system',
    explanation_level: 'basic',
    compact_mode: false,
  };
  return {
    windowSetCompact: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    appSettingsGet: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { general: sampleGeneral },
    }),
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: { windowSetCompact, appSettingsGet, appSettingsSetGeneral },
}));

import {
  compactState,
  initCompact,
  observeCompactContent,
  setCompact,
  toggleCompact,
} from '$lib/layout/compact.svelte';

/**
 * Build a fake compact layout in the (jsdom) document so the height measurement
 * has something to read. jsdom does no layout, so `getBoundingClientRect`
 * returns zeros by default — we stub it per element. `phaseHeight === null`
 * omits the status row entirely (it's unmounted when no install is running).
 */
function mountCompactDom({
  sidebarBottom,
  phaseHeight,
}: {
  sidebarBottom: number;
  phaseHeight: number | null;
}): { phaseRow: HTMLElement | null } {
  document.body.innerHTML = '';
  const aside = document.createElement('aside');
  aside.setAttribute('data-sidebar', '');
  aside.getBoundingClientRect = () => ({ top: 0 }) as DOMRect;
  const child = document.createElement('div');
  child.getBoundingClientRect = () => ({ bottom: sidebarBottom }) as DOMRect;
  aside.appendChild(child);
  document.body.appendChild(aside);

  let phaseRow: HTMLElement | null = null;
  if (phaseHeight !== null) {
    phaseRow = document.createElement('div');
    phaseRow.setAttribute('data-phase-row', '');
    phaseRow.getBoundingClientRect = () => ({ height: phaseHeight }) as DOMRect;
    document.body.appendChild(phaseRow);
  }
  return { phaseRow };
}

describe('compact mode rune module', () => {
  beforeEach(() => {
    compactState.value = false;
    document.body.innerHTML = '';
    windowSetCompact.mockClear();
    appSettingsGet.mockClear();
    appSettingsSetGeneral.mockClear();
  });

  it('setCompact flips the rune, resizes the window, and persists the flag', async () => {
    await setCompact(true);
    expect(compactState.value).toBe(true);
    // Height is null here: jsdom has no rendered sidebar to measure.
    expect(windowSetCompact).toHaveBeenCalledWith(true, null);
    expect(appSettingsSetGeneral).toHaveBeenCalledTimes(1);
    expect(appSettingsSetGeneral.mock.calls[0][0]).toMatchObject({ compact_mode: true });
  });

  it('toggleCompact inverts the current value', async () => {
    compactState.value = false;
    await toggleCompact();
    expect(compactState.value).toBe(true);
    expect(windowSetCompact).toHaveBeenLastCalledWith(true, null);
  });

  it('initCompact applies the persisted mode WITHOUT re-persisting', async () => {
    await initCompact(true);
    expect(compactState.value).toBe(true);
    expect(windowSetCompact).toHaveBeenCalledWith(true, null);
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });

  it('initCompact(false) applies expanded constraints without persisting', async () => {
    await initCompact(false);
    expect(compactState.value).toBe(false);
    // Still calls the backend (to apply the min-height floor) but does not persist.
    expect(windowSetCompact).toHaveBeenCalledWith(false, null);
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });

  it('setCompact rolls the rune back and does not persist when the resize fails', async () => {
    windowSetCompact.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'window_io', details: 'x' },
    });
    await setCompact(true);
    expect(compactState.value).toBe(false); // rolled back
    expect(appSettingsSetGeneral).not.toHaveBeenCalled();
  });

  it('sizes the compact window to the sidebar PLUS the status row', async () => {
    // Status row (28px) is below the sidebar in grid row 2 — its height must be
    // added, or the strip is too short and the sidebar grows a scrollbar.
    mountCompactDom({ sidebarBottom: 400, phaseHeight: 28 });
    await setCompact(true);
    expect(windowSetCompact).toHaveBeenCalledWith(true, 428);
  });

  it('sizes to the sidebar alone when no status row is rendered', async () => {
    mountCompactDom({ sidebarBottom: 400, phaseHeight: null });
    await setCompact(true);
    expect(windowSetCompact).toHaveBeenCalledWith(true, 400);
  });
});

describe('compact auto-resize observer', () => {
  let resizeCallback: ResizeObserverCallback | null;
  let observed: Element[];

  beforeEach(() => {
    compactState.value = false;
    document.body.innerHTML = '';
    windowSetCompact.mockClear();
    resizeCallback = null;
    observed = [];
    // jsdom has no ResizeObserver — capture the callback so tests can drive it.
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: ResizeObserverCallback) {
          resizeCallback = cb;
        }
        observe(el: Element) {
          observed.push(el);
        }
        disconnect() {}
        unobserve() {}
      },
    );
  });

  it('re-applies the window height when content changes while compact', () => {
    const { phaseRow } = mountCompactDom({ sidebarBottom: 400, phaseHeight: 0 });
    compactState.value = true;
    const dispose = observeCompactContent();
    expect(observed.length).toBeGreaterThan(0);

    // Establish the baseline height (400) regardless of prior module state, so
    // the assertion below tests the *change*, not residual state.
    resizeCallback?.([], {} as ResizeObserver);
    windowSetCompact.mockClear();

    // Status row appears (0 -> 28): the observer should grow the window.
    phaseRow!.getBoundingClientRect = () => ({ height: 28 }) as DOMRect;
    resizeCallback?.([], {} as ResizeObserver);
    expect(windowSetCompact).toHaveBeenCalledWith(true, 428);

    // A second tick with no change must not re-resize (no feedback loop).
    resizeCallback?.([], {} as ResizeObserver);
    expect(windowSetCompact).toHaveBeenCalledTimes(1);

    dispose();
  });

  it('stays idle while expanded', () => {
    mountCompactDom({ sidebarBottom: 400, phaseHeight: 28 });
    compactState.value = false;
    const dispose = observeCompactContent();
    windowSetCompact.mockClear();

    resizeCallback?.([], {} as ResizeObserver);
    expect(windowSetCompact).not.toHaveBeenCalled();

    dispose();
  });
});
