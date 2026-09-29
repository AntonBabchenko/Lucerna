import { afterEach, describe, expect, it } from 'vitest';
import { stickyEdge } from '$lib/ui/sticky-edge';

// Plan §5b V2 (screenshot 01f, WCAG 2.4.11 Focus Not Obscured): Tab moved focus onto a row the
// sticky Installed toolbar covered — the browser scrolls a focused control just into the
// scrollport, and the toolbar sits on top of the scrollport's first 80 px. A sticky bar therefore
// reserves its own height, plus room for the focus ring, as its scroll container's
// scroll-padding — which focus scrolling and `scrollIntoView` both honour.

/** A scroll container (inline style: happy-dom computes no Tailwind) holding one sticky bar. */
function scrollerWith(...heights: number[]) {
  const scroller = document.createElement('div');
  scroller.style.overflowY = 'auto';
  const bars = heights.map((h) => {
    const bar = document.createElement('div');
    bar.getBoundingClientRect = () => ({ height: h, top: 0, bottom: h }) as DOMRect;
    scroller.append(bar);
    return bar;
  });
  document.body.append(scroller);
  return { scroller, bars };
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('stickyEdge', () => {
  it('reserves a header’s height, and room for the focus ring, as scroll-padding-top', () => {
    const { scroller, bars } = scrollerWith(84);
    const action = stickyEdge(bars[0] as HTMLElement);
    expect(scroller.style.scrollPaddingTop).toBe('92px');
    action.destroy();
    expect(scroller.style.scrollPaddingTop).toBe('');
  });

  it('a footer reserves scroll-padding-bottom', () => {
    const { scroller, bars } = scrollerWith(40);
    stickyEdge(bars[0] as HTMLElement, 'bottom');
    expect(scroller.style.scrollPaddingBottom).toBe('48px');
    expect(scroller.style.scrollPaddingTop).toBe('');
  });

  // Add-ons keeps Browse and Installed mounted in one scroll container, one of them hidden: a
  // hidden bar measures 0 and must not take the room the shown one needs, whichever registers last.
  it('several bars on one edge reserve the tallest; a hidden one takes nothing', () => {
    const { scroller, bars } = scrollerWith(84, 0);
    const shown = stickyEdge(bars[0] as HTMLElement);
    stickyEdge(bars[1] as HTMLElement);
    expect(scroller.style.scrollPaddingTop).toBe('92px');
    shown.destroy();
    expect(scroller.style.scrollPaddingTop).toBe('');
  });

  it('outside a scroll container it touches nothing', () => {
    const outer = document.createElement('div');
    const bar = document.createElement('div');
    bar.getBoundingClientRect = () => ({ height: 84 }) as DOMRect;
    outer.append(bar);
    document.body.append(outer);
    expect(() => stickyEdge(bar).destroy()).not.toThrow();
    expect(outer.style.scrollPaddingTop).toBe('');
  });
});
