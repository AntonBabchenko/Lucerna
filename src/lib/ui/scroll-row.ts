/**
 * A line that scrolls sideways instead of wrapping (DESIGN.md §6): the Installed toolbar's filter
 * chips. At the launcher's default 820 px they wrapped to two lines of a sticky toolbar that took
 * four, and left the rows ~170 px of the window (plan 2026-09-28 §5c V3, screenshots n01, n01e).
 *
 * `use:scrollRow` on a `.scroll-row` box (app.css: `overflow-x: auto`, a thin scrollbar, a fade on
 * each side that has more):
 * - says which sides have more to scroll to — `data-more-start` / `data-more-end` — which the CSS
 *   fades: the affordance where the platform's scrollbar hides at rest;
 * - scrolls a focused descendant clear of the fade: Tab and the arrow keys move focus along the
 *   line, and a chip focused under the fade, or past the edge, would be hidden;
 * - with `keepInView`, keeps the element that selector finds — the chosen chip — in view when it
 *   changes: a filter chosen from elsewhere (↗ in the problems panel picks «Issues») must not sit
 *   scrolled out of the line.
 * Only this box scrolls: never `scrollIntoView`, which would move the list under a sticky toolbar
 * too.
 */

/** The fade's width in the CSS (`.scroll-row`, 2rem): a revealed item keeps this much room. */
export const SCROLL_ROW_FADE_PX = 32;

export type ScrollRowOptions = {
  /** A selector for the element to keep in view, re-checked when the line changes. */
  keepInView?: string;
};

/**
 * Where a line scrolled to `view` must scroll so an item — `left` in the line's own content
 * coordinates — shows whole, with `margin` to spare on both sides. Unchanged when it already does;
 * an item wider than the view shows its start.
 */
export function revealScrollLeft(
  view: { scrollLeft: number; clientWidth: number },
  item: { left: number; width: number },
  margin: number,
): number {
  const start = item.left - margin;
  const end = item.left + item.width + margin;
  if (start < view.scrollLeft) return Math.max(0, start);
  if (end > view.scrollLeft + view.clientWidth) {
    return Math.max(0, Math.min(start, end - view.clientWidth));
  }
  return view.scrollLeft;
}

/** Room to scroll either way, give or take a pixel of subpixel layout. */
function markMore(node: HTMLElement): void {
  const max = node.scrollWidth - node.clientWidth;
  node.toggleAttribute('data-more-start', node.scrollLeft > 1);
  node.toggleAttribute('data-more-end', node.scrollLeft < max - 1);
}

function reveal(node: HTMLElement, el: Element): void {
  const port = node.getBoundingClientRect();
  const box = el.getBoundingClientRect();
  const left = box.left - port.left - node.clientLeft + node.scrollLeft;
  // A layout-less measure (a hidden line, a test DOM) reveals nothing.
  if (!Number.isFinite(left) || !Number.isFinite(box.width)) return;
  const next = revealScrollLeft(node, { left, width: box.width }, SCROLL_ROW_FADE_PX);
  if (next !== node.scrollLeft) node.scrollLeft = next;
  markMore(node);
}

export function scrollRow(node: HTMLElement, opts: ScrollRowOptions = {}) {
  let keepInView = opts.keepInView ?? null;
  const mark = () => markMore(node);
  const revealKept = () => {
    const el = keepInView ? node.querySelector(keepInView) : null;
    if (el) reveal(node, el);
  };
  const onFocusIn = (e: FocusEvent) => {
    if (e.target instanceof Element && e.target !== node) reveal(node, e.target);
  };

  node.addEventListener('scroll', mark, { passive: true });
  node.addEventListener('focusin', onFocusIn);
  // The line's width follows the window — a narrower one can leave the chosen chip past its edge;
  // its content's, the chips it lists and their counts, which move nothing into view: a count
  // that changes must not scroll the line back from where the user took it.
  const resize =
    typeof ResizeObserver === 'undefined'
      ? null
      : new ResizeObserver((entries) => {
          if (entries.some((e) => e.target === node)) revealKept();
          mark();
        });
  resize?.observe(node);
  for (const child of node.children) resize?.observe(child);
  // A chip added or removed, or another one chosen.
  const mutation =
    typeof MutationObserver === 'undefined'
      ? null
      : new MutationObserver(() => {
          revealKept();
          mark();
        });
  mutation?.observe(node, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ['aria-checked'],
  });
  revealKept();
  mark();

  return {
    update(next: ScrollRowOptions = {}) {
      keepInView = next.keepInView ?? null;
      revealKept();
    },
    destroy() {
      resize?.disconnect();
      mutation?.disconnect();
      node.removeEventListener('scroll', mark);
      node.removeEventListener('focusin', onFocusIn);
    },
  };
}
