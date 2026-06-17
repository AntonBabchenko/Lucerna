import { beforeEach, describe, expect, it, vi } from 'vitest';

const { windowSetCompact, windowSetExpandedFloor, appSettingsGet, appSettingsSetGeneral } =
  vi.hoisted(() => {
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
      windowSetExpandedFloor: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
      appSettingsSetGeneral: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
      appSettingsGet: vi.fn().mockResolvedValue({
        status: 'ok',
        data: { general: sampleGeneral },
      }),
    };
  });

vi.mock('$lib/ipc/bindings', () => ({
  commands: { windowSetCompact, windowSetExpandedFloor, appSettingsGet, appSettingsSetGeneral },
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
    windowSetExpandedFloor.mockClear();
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

  it('initCompact(false) does not send a premature null floor', async () => {
    await initCompact(false);
    expect(compactState.value).toBe(false);
    // jsdom has no laid-out sidebar → measure is null → nothing sent yet; the
    // observer applies (and arms) the floor once the content is measurable.
    expect(windowSetExpandedFloor).not.toHaveBeenCalled();
    expect(windowSetCompact).not.toHaveBeenCalled();
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
    windowSetExpandedFloor.mockClear();
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

  it('arms the expanded floor on the first fire, then re-applies on content change', () => {
    // Deferred startup: measure is null at init (no DOM yet), so the hug is
    // pending and must be performed by the observer's first post-layout fire.
    void initCompact(false);
    mountCompactDom({ sidebarBottom: 300, phaseHeight: null });
    const child = document.querySelector('[data-sidebar]')!.firstElementChild as HTMLElement;
    compactState.value = false;
    const dispose = observeCompactContent();
    expect(observed.length).toBeGreaterThan(0);

    // First fire after layout → hug=true (arms via set_size at content height).
    resizeCallback?.([], {} as ResizeObserver);
    expect(windowSetExpandedFloor).toHaveBeenLastCalledWith(300, true);
    expect(windowSetCompact).not.toHaveBeenCalled();
    windowSetExpandedFloor.mockClear();

    // Content grows (e.g. a live locale switch) → re-apply with hug=false.
    child.getBoundingClientRect = () => ({ bottom: 360 }) as DOMRect;
    resizeCallback?.([], {} as ResizeObserver);
    expect(windowSetExpandedFloor).toHaveBeenLastCalledWith(360, false);

    // Unchanged height → no redundant call (no feedback loop).
    resizeCallback?.([], {} as ResizeObserver);
    expect(windowSetExpandedFloor).toHaveBeenCalledTimes(1);

    dispose();
  });

  it('expanded floor excludes the status row — no window growth when an install footer appears', () => {
    // Variant A: in expanded mode the sidebar spans BOTH grid rows, so the
    // status row sits only under the content column and never steals sidebar
    // height. The floor is measured from the sidebar alone, and the status row
    // appearing must NOT grow the window (which previously squeezed the sidebar
    // into a scrollbar). Contrast the compact case above, where it grows to 428.
    void initCompact(false); // arm hug deterministically; measure is null (no DOM yet)
    const { phaseRow } = mountCompactDom({ sidebarBottom: 400, phaseHeight: 0 });
    compactState.value = false;
    const dispose = observeCompactContent();

    // First fire establishes the floor at the sidebar height ALONE (400, not 428).
    resizeCallback?.([], {} as ResizeObserver);
    expect(windowSetExpandedFloor).toHaveBeenCalledTimes(1);
    expect(windowSetExpandedFloor.mock.calls.at(-1)?.[0]).toBe(400);
    windowSetExpandedFloor.mockClear();

    // Status row appears (0 -> 28). Expanded mode must IGNORE it: no resize,
    // because the sidebar spans the full height regardless of the footer.
    phaseRow!.getBoundingClientRect = () => ({ height: 28 }) as DOMRect;
    resizeCallback?.([], {} as ResizeObserver);
    expect(windowSetExpandedFloor).not.toHaveBeenCalled();

    dispose();
  });
});
