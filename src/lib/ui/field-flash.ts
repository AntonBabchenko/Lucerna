/**
 * How long the accent highlight stays on a flashed field. Also the length of its
 * fade-in / blink / fade-out animation: the action hands this value to the CSS
 * as `--field-flash-duration`, so the two cannot drift.
 */
export const FLASH_MS = 2800;

/**
 * Anything a user can tab to. Used to find the control inside the wrapper the
 * action is applied to — the action wraps a label + control pair, not the bare
 * control, so the highlight covers both.
 */
const FOCUSABLE = 'button, input, textarea, [href], [tabindex]:not([tabindex="-1"])';

export type FieldFlashParams = {
  /** True while this field is the one the user asked to be taken to. */
  active: boolean;
  /**
   * Also move keyboard focus into the field. Off by default: on a range slider
   * a focus grab turns the next arrow key into a silent value edit.
   */
  focus?: boolean;
  /**
   * Called once the flash has been delivered (scroll, highlight, focus arranged) —
   * in a microtask, never inside the action's own call, so a wrapper that
   * mounts already active still records the edge it just took.
   */
  onDelivered?: () => void;
  /** How the field is scrolled into view; the browser's default when unset. */
  behavior?: ScrollBehavior;
};

function isDisabled(el: HTMLElement): boolean {
  return (el as HTMLInputElement).disabled === true;
}

/** The nearest scrolling ancestor, else the document element. */
function scrollParentOf(node: HTMLElement): HTMLElement {
  let el = node.parentElement;
  while (el) {
    const overflowY = getComputedStyle(el).overflowY;
    if (overflowY === 'auto' || overflowY === 'scroll') return el;
    el = el.parentElement;
  }
  return document.documentElement;
}

/**
 * Open every closed <details> between `target` and `root`: focus() on an
 * element inside a closed disclosure does nothing, and the wrapper's height
 * is only right once it is open.
 */
function openDisclosures(target: HTMLElement, root: HTMLElement): void {
  let details = target.closest('details');
  while (details && details !== root && root.contains(details)) {
    details.open = true;
    details = details.parentElement?.closest('details') ?? null;
  }
}

/**
 * The control to focus: a marked one, else the first usable focusable, else
 * the first disabled one — to wait for. A field whose controls are all
 * disabled (their settings still loading) must still hold focus: the link that
 * asked for it may have unmounted, and with no target focus would fall to
 * <body>, outside any dialog.
 */
function focusTargetIn(node: HTMLElement): HTMLElement | null {
  const preferred = node.querySelector<HTMLElement>('[data-flash-focus]');
  if (preferred) return preferred;
  let firstDisabled: HTMLElement | null = null;
  for (const el of node.querySelectorAll<HTMLElement>(FOCUSABLE)) {
    if (!isDisabled(el)) return el;
    firstDisabled ??= el;
  }
  return firstDisabled;
}

/** What a user does to take the scroll themselves; programmatic scrolls fire none of these. */
const USER_SCROLL_EVENTS = ['wheel', 'touchstart', 'pointerdown', 'keydown'] as const;

/**
 * Svelte action. On the `active: false → true` edge it opens any disclosure
 * around the focus target, scrolls the node into view (a block taller than
 * its scrollport aligns to its start, so a long block shows its heading, not
 * its middle), paints `.field-flash` for FLASH_MS, and — only when asked —
 * focuses the target, waiting for a disabled one to enable. While the flash
 * runs it keeps the node aligned as the layout settles (a section still
 * loading, content above it growing), until the user scrolls themselves.
 * Deactivation lets a running highlight finish; only destroy cuts it short.
 */
export function fieldFlash(node: HTMLElement, params: FieldFlashParams) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let wasActive = false;
  let enableWatch: MutationObserver | null = null;
  let settleWatch: ResizeObserver | null = null;
  let stopUserWatch: (() => void) | null = null;

  function stopWatch() {
    enableWatch?.disconnect();
    enableWatch = null;
  }

  function stopSettle() {
    settleWatch?.disconnect();
    settleWatch = null;
    stopUserWatch?.();
    stopUserWatch = null;
  }

  function clear() {
    if (timer) clearTimeout(timer);
    timer = null;
    stopWatch();
    stopSettle();
    node.classList.remove('field-flash');
  }

  function align(p: FieldFlashParams) {
    const taller = node.getBoundingClientRect().height > scrollParentOf(node).clientHeight;
    // happy-dom has no layout, so scrollIntoView is absent there.
    node.scrollIntoView?.({
      block: taller ? 'start' : 'center',
      ...(p.behavior ? { behavior: p.behavior } : {}),
    });
  }

  /**
   * The flash is delivered when the field mounts, which can be before what it
   * sits in has loaded: content above it grows, or its own rows arrive, and a
   * single scroll leaves it off-screen. Re-align on every size change of the
   * node or of the scroll container's children until the flash ends or the
   * user scrolls.
   */
  function keepAligned(p: FieldFlashParams) {
    if (typeof ResizeObserver !== 'function') return;
    const scroller = scrollParentOf(node);
    settleWatch = new ResizeObserver(() => align(p));
    settleWatch.observe(node);
    for (const child of Array.from(scroller.children)) {
      if (child !== node) settleWatch.observe(child);
    }
    const userTookOver = () => stopSettle();
    for (const ev of USER_SCROLL_EVENTS) {
      scroller.addEventListener(ev, userTookOver, { capture: true, passive: true });
    }
    stopUserWatch = () => {
      for (const ev of USER_SCROLL_EVENTS) scroller.removeEventListener(ev, userTookOver, { capture: true });
    };
  }

  function focusWhenUsable(target: HTMLElement) {
    if (!isDisabled(target)) {
      target.focus({ preventScroll: true });
      return;
    }
    // A field that is disabled until a read answers (the CurseForge key input
    // while its status loads). Park focus on the wrapper meanwhile — inside
    // the dialog, so the dialog's own deferred initial focus leaves it alone —
    // and move it into the field the moment it enables, but only if nothing
    // else has claimed focus since: a user already typing elsewhere is never
    // yanked. A field that never enables is never grabbed.
    if (typeof MutationObserver !== 'function') return;
    if (!node.hasAttribute('tabindex')) node.tabIndex = -1;
    node.focus({ preventScroll: true });
    stopWatch();
    enableWatch = new MutationObserver(() => {
      if (isDisabled(target)) return;
      stopWatch();
      const held = document.activeElement;
      if (held === node || held === document.body || held === null) {
        target.focus({ preventScroll: true });
      }
    });
    enableWatch.observe(target, { attributes: true, attributeFilter: ['disabled'] });
  }

  function flash(p: FieldFlashParams) {
    clear();
    const target = p.focus ? focusTargetIn(node) : null;
    // Open first: the height below is only right once the disclosure is.
    if (target) openDisclosures(target, node);
    // This layout read also flushes clear()'s class removal, so a re-flash
    // while a highlight still runs restarts its animation instead of continuing it.
    align(p);
    keepAligned(p);
    node.style.setProperty('--field-flash-duration', `${FLASH_MS}ms`);
    node.classList.add('field-flash');
    timer = setTimeout(() => {
      timer = null;
      stopWatch();
      stopSettle();
      node.classList.remove('field-flash');
    }, FLASH_MS);
    if (target) focusWhenUsable(target);
    if (p.onDelivered) queueMicrotask(p.onDelivered);
  }

  function apply(p: FieldFlashParams) {
    // Only the false → true edge flashes; deactivation lets a running highlight end.
    if (p.active && !wasActive) flash(p);
    wasActive = p.active;
  }

  apply(params);

  return { update: apply, destroy: clear };
}
