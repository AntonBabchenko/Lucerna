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

/**
 * Svelte action. On the `active: false → true` edge it opens any disclosure
 * around the focus target, scrolls the node into view (a block taller than
 * its scrollport aligns to its start, so a long block shows its heading, not
 * its middle), paints `.field-flash` for FLASH_MS, and — only when asked —
 * focuses the target, waiting for a disabled one to enable. Deactivation lets
 * a running highlight finish; only destroy cuts it short.
 */
export function fieldFlash(node: HTMLElement, params: FieldFlashParams) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let wasActive = false;
  let enableWatch: MutationObserver | null = null;

  function stopWatch() {
    enableWatch?.disconnect();
    enableWatch = null;
  }

  function clear() {
    if (timer) clearTimeout(timer);
    timer = null;
    stopWatch();
    node.classList.remove('field-flash');
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
    const taller = node.getBoundingClientRect().height > scrollParentOf(node).clientHeight;
    // happy-dom has no layout, so scrollIntoView is absent there.
    node.scrollIntoView?.({ block: taller ? 'start' : 'center' });
    node.style.setProperty('--field-flash-duration', `${FLASH_MS}ms`);
    node.classList.add('field-flash');
    timer = setTimeout(() => {
      timer = null;
      stopWatch();
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
