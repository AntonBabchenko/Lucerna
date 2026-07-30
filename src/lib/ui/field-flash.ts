/** How long the accent ring stays on a flashed field. */
export const FLASH_MS = 2000;

/**
 * Anything a user can tab to. Used to find the control inside the wrapper the
 * action is applied to — the action wraps a label + control pair, not the bare
 * control, so the ring encloses both.
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
};

/**
 * Svelte action. On the `active: false → true` edge it scrolls the node into
 * view, paints `.field-flash` for FLASH_MS, and — only when asked — focuses the
 * first focusable descendant.
 */
export function fieldFlash(node: HTMLElement, params: FieldFlashParams) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let wasActive = false;

  function clear() {
    if (timer) clearTimeout(timer);
    timer = null;
    node.classList.remove('field-flash');
  }

  function flash(p: FieldFlashParams) {
    clear();
    // happy-dom has no layout, so scrollIntoView is absent there.
    node.scrollIntoView?.({ block: 'center' });
    node.classList.add('field-flash');
    timer = setTimeout(() => {
      timer = null;
      node.classList.remove('field-flash');
    }, FLASH_MS);
    if (p.focus) {
      node.querySelector<HTMLElement>(FOCUSABLE)?.focus({ preventScroll: true });
    }
  }

  function apply(p: FieldFlashParams) {
    if (p.active && !wasActive) flash(p);
    else if (!p.active && wasActive) clear();
    wasActive = p.active;
  }

  apply(params);

  return { update: apply, destroy: clear };
}
