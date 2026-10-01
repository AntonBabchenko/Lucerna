/**
 * Sticky list chrome keeps what it covers reachable (DESIGN.md §14). A browser scrolls a focused
 * control only just into its scroll container's scrollport — and a sticky toolbar sits on top of
 * that scrollport's first line, so Tab could land on a row nobody can see (WCAG 2.4.11, Focus Not
 * Obscured; plan 2026-09-28 §5b V2, screenshot 01f). A sticky bar therefore reserves its own
 * height, plus room for the focus ring, as its scroll container's `scroll-padding` on its edge:
 * focus scrolling and `scrollIntoView` both keep clear of it.
 *
 * One scroll container may hold several bars on one edge — Add-ons keeps Browse and Installed
 * mounted, the hidden one measuring 0 — so each edge reserves the tallest bar on it, whichever
 * measured last.
 *
 * A bar also says when it is stuck — content scrolls under it — as `data-stuck`, so it can show
 * an edge only then (`data-[stuck]:` in Tailwind): rows cut mid-glyph under a bar with no edge read
 * as broken (plan §5b V2, screenshot 01e).
 */
export type StickyEdge = 'top' | 'bottom';

/** Room kept between a bar and the control scrolled next to it: the 2 px focus ring at its 2 px
 *  offset shows in full, with the gap the toolbar leaves above the list at rest. */
export const STICKY_GAP_PX = 8;

type Bar = { edge: StickyEdge; size: number };

// Per scroll container, its bars — a WeakMap, so a container that goes away takes its entry along.
const barsByScroller = new WeakMap<HTMLElement, Map<HTMLElement, Bar>>();

/** The nearest ancestor that scrolls vertically — the box a sticky bar sticks to. */
function scrollContainerOf(node: HTMLElement): HTMLElement | null {
  for (let el = node.parentElement; el; el = el.parentElement) {
    const { overflowY } = getComputedStyle(el);
    if (overflowY === 'auto' || overflowY === 'scroll' || overflowY === 'overlay') return el;
  }
  return null;
}

function reserve(scroller: HTMLElement): void {
  const bars = [...(barsByScroller.get(scroller)?.values() ?? [])];
  const tallest = (edge: StickyEdge) =>
    Math.max(0, ...bars.filter((b) => b.edge === edge).map((b) => b.size));
  const padding = (size: number) => (size > 0 ? `${Math.ceil(size) + STICKY_GAP_PX}px` : '');
  scroller.style.scrollPaddingTop = padding(tallest('top'));
  scroller.style.scrollPaddingBottom = padding(tallest('bottom'));
}

/** Stuck = pinned to its edge of the scrollport with content scrolled under it. A bar that has
 *  only started to move with the page (the padding above it scrolling away) is not stuck yet. */
function isStuck(node: HTMLElement, scroller: HTMLElement, edge: StickyEdge): boolean {
  const bar = node.getBoundingClientRect();
  if (bar.height === 0) return false;
  const port = scroller.getBoundingClientRect();
  if (edge === 'top') return scroller.scrollTop > 0 && bar.top <= port.top + 1;
  const moreBelow = scroller.scrollTop + scroller.clientHeight < scroller.scrollHeight - 1;
  return moreBelow && bar.bottom >= port.bottom - 1;
}

/**
 * `use:stickyEdge` on a `sticky top-0` bar (or `use:stickyEdge={'bottom'}` on a `sticky bottom-0`
 * pager). Outside a scroll container it does nothing: nothing scrolls under the bar there.
 */
export function stickyEdge(node: HTMLElement, edge: StickyEdge = 'top') {
  const scroller = scrollContainerOf(node);
  if (!scroller) return { destroy() {} };
  const bars = barsByScroller.get(scroller) ?? new Map<HTMLElement, Bar>();
  barsByScroller.set(scroller, bars);
  const bar: Bar = { edge, size: 0 };
  bars.set(node, bar);
  const markStuck = () => node.toggleAttribute('data-stuck', isStuck(node, scroller, edge));
  const measure = () => {
    // 0 while the bar is not rendered (a hidden view): it then takes no room.
    bar.size = node.getBoundingClientRect().height;
    reserve(scroller);
    markStuck();
  };
  measure();
  // Its height follows the window width (the chips wrap) and its view being shown or hidden.
  const resize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
  resize?.observe(node);
  scroller.addEventListener('scroll', markStuck, { passive: true });
  return {
    destroy() {
      resize?.disconnect();
      scroller.removeEventListener('scroll', markStuck);
      bars.delete(node);
      reserve(scroller);
    },
  };
}
