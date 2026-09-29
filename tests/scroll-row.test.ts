import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { revealScrollLeft, SCROLL_ROW_FADE_PX, scrollRow } from '$lib/ui/scroll-row';

// Plan §5c V3 (screenshots n01, n01e): at the launcher's default 820 px the Installed toolbar's
// filter chips wrapped to two lines under a sticky toolbar that already took four, and left the
// rows ~170 px. The chips keep to one line that scrolls sideways instead: a fade says which side
// has more, a focused chip is scrolled clear of the fade, and the chosen one is kept in view.

// Plan §5d L1: the fades stopped 4 px short of the line's edges. They are sticky inside the line,
// and a sticky box keeps to its scroll container's padding — the 4 px the line pads itself by for
// a focused chip's ring. So the box owns that padding, and each fade is inset by minus it. happy-dom
// computes no layout, so the rule is read from app.css (as tests/intent/browser-feel.test.ts does).
describe('.scroll-row fades', () => {
  const css = readFileSync(resolve(process.cwd(), 'src/app.css'), 'utf8').replace(
    /\/\*[\s\S]*?\*\//g,
    '',
  );
  // The body of the rule whose whole selector is `selector` — not a group that lists it.
  const block = (selector: string) =>
    [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].find((m) => m[1]?.trim() === selector)?.[2] ?? '';
  const prop = (body: string, name: string) =>
    body.match(new RegExp(`(?:^|[;\\s])${name}\\s*:\\s*([^;]+);`))?.[1]?.trim() ?? null;

  it('reach the visible edges of the line, past the room it keeps for a focus ring', () => {
    const line = block('.scroll-row');
    const pad = prop(line, 'padding');
    expect(pad).toMatch(/^\d*\.?\d+(rem|px)$/);
    // The room is given back, so the line sits where its chips would.
    expect(prop(line, 'margin')).toBe(`-${pad}`);
    expect(prop(block('.scroll-row::before'), 'left')).toBe(`-${pad}`);
    expect(prop(block('.scroll-row::after'), 'right')).toBe(`-${pad}`);
  });
});

describe('revealScrollLeft', () => {
  const view = { scrollLeft: 100, clientWidth: 300 };

  it('leaves a line alone when the item is in view, clear of the fade', () => {
    expect(revealScrollLeft(view, { left: 200, width: 80 }, 32)).toBe(100);
  });

  it('scrolls back to show an item cut at the start, clear of the fade', () => {
    expect(revealScrollLeft(view, { left: 110, width: 80 }, 32)).toBe(78);
  });

  it('scrolls on to show an item cut at the end, clear of the fade', () => {
    // Its end at 390 + 32 of room: the view must reach 422, so it starts at 122.
    expect(revealScrollLeft(view, { left: 310, width: 80 }, 32)).toBe(122);
  });

  it('shows the start of an item wider than the view', () => {
    expect(revealScrollLeft(view, { left: 500, width: 400 }, 32)).toBe(468);
  });

  it('never scrolls before the start of the line', () => {
    expect(revealScrollLeft(view, { left: 4, width: 60 }, 32)).toBe(0);
  });
});

/** A scrolling line in happy-dom, which lays nothing out: its geometry is set by hand. */
function line(opts: { clientWidth: number; scrollWidth: number; chips: [number, number][] }) {
  const node = document.createElement('div');
  let scrollLeft = 0;
  Object.defineProperty(node, 'scrollLeft', {
    configurable: true,
    get: () => scrollLeft,
    set: (v: number) => {
      scrollLeft = v;
    },
  });
  Object.defineProperty(node, 'clientWidth', { configurable: true, value: opts.clientWidth });
  Object.defineProperty(node, 'scrollWidth', { configurable: true, value: opts.scrollWidth });
  node.getBoundingClientRect = () => ({ left: 0, width: opts.clientWidth }) as DOMRect;
  const group = document.createElement('div');
  node.append(group);
  const chips = opts.chips.map(([left, width]) => {
    const chip = document.createElement('button');
    chip.getBoundingClientRect = () => ({ left: left - scrollLeft, width }) as DOMRect;
    group.append(chip);
    return chip;
  });
  document.body.append(node);
  return { node, chips, scrollTo: (v: number) => (scrollLeft = v) };
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('scrollRow', () => {
  it('says which side has more to scroll to, and follows the scroll', () => {
    const { node, scrollTo } = line({ clientWidth: 300, scrollWidth: 700, chips: [] });
    const action = scrollRow(node);
    expect(node.hasAttribute('data-more-start')).toBe(false);
    expect(node.hasAttribute('data-more-end')).toBe(true);
    scrollTo(200);
    node.dispatchEvent(new Event('scroll'));
    expect(node.hasAttribute('data-more-start')).toBe(true);
    expect(node.hasAttribute('data-more-end')).toBe(true);
    scrollTo(400);
    node.dispatchEvent(new Event('scroll'));
    expect(node.hasAttribute('data-more-start')).toBe(true);
    expect(node.hasAttribute('data-more-end')).toBe(false);
    action.destroy();
  });

  it('marks nothing when the line fits', () => {
    const { node } = line({ clientWidth: 700, scrollWidth: 700, chips: [] });
    scrollRow(node);
    expect(node.hasAttribute('data-more-start')).toBe(false);
    expect(node.hasAttribute('data-more-end')).toBe(false);
  });

  // Tab and the arrow keys move focus along the line: a chip focused under the fade, or past the
  // edge, is scrolled into view with the fade's width to spare.
  it('scrolls a focused chip clear of the fade', () => {
    const { node, chips } = line({
      clientWidth: 300,
      scrollWidth: 700,
      chips: [
        [0, 80],
        [290, 90],
      ],
    });
    scrollRow(node);
    chips[1]?.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    expect(node.scrollLeft).toBe(290 + 90 + SCROLL_ROW_FADE_PX - 300);
    expect(node.hasAttribute('data-more-start')).toBe(true);
  });

  // The chosen filter must stay in sight: picking one from elsewhere (↗ in the problems panel sets
  // «Issues») would otherwise leave it scrolled out of the line.
  it('keeps the chosen chip in view when the choice changes', async () => {
    const { node, chips } = line({
      clientWidth: 300,
      scrollWidth: 700,
      chips: [
        [0, 80],
        [500, 100],
      ],
    });
    chips[0]?.setAttribute('aria-checked', 'true');
    chips[1]?.setAttribute('aria-checked', 'false');
    scrollRow(node, { keepInView: '[aria-checked="true"]' });
    expect(node.scrollLeft).toBe(0);
    chips[0]?.setAttribute('aria-checked', 'false');
    chips[1]?.setAttribute('aria-checked', 'true');
    await Promise.resolve();
    expect(node.scrollLeft).toBe(500 + 100 + SCROLL_ROW_FADE_PX - 300);
  });
});
