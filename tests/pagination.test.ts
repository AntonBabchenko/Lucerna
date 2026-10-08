import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import PageSizePicker from '$lib/mods/PageSizePicker.svelte';
import Pagination from '$lib/ui/Pagination.svelte';
import {
  hideTooltip,
  OPEN_DELAY_MS,
  tooltipState,
} from '$lib/ui/tooltip/tooltip-controller.svelte';
import { dismissTooltip, revealTooltip } from './test-utils/reveal-tooltip';

// Shared pagination control used by every browser (mods / RP / shaders,
// modpacks, installed). Behaviour is index-based: `page` is 0-based, the
// component emits the target index via onPage; the container clamps + reloads.

const base = {
  page: 0,
  pageCount: 5,
  onPage: () => {},
};

describe('Pagination', () => {
  it('renders first, previous, next and last steps and a page label', () => {
    render(Pagination, { props: { ...base } });
    expect(screen.getByTestId('pg-first')).toBeTruthy();
    expect(screen.getByTestId('pg-prev')).toBeTruthy();
    expect(screen.getByTestId('pg-next')).toBeTruthy();
    expect(screen.getByTestId('pg-last')).toBeTruthy();
    // 0-based page 0 renders as "1 of 5" for humans.
    expect(screen.getByTestId('pg-label').textContent).toMatch(/1.*5/);
  });

  it('disables First/Prev on the first page, enables Next/Last', () => {
    render(Pagination, { props: { ...base, page: 0, pageCount: 5 } });
    expect((screen.getByTestId('pg-first') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('pg-prev') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('pg-next') as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByTestId('pg-last') as HTMLButtonElement).disabled).toBe(false);
  });

  it('disables Next/Last on the last page, enables First/Prev', () => {
    render(Pagination, { props: { ...base, page: 4, pageCount: 5 } });
    expect((screen.getByTestId('pg-first') as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByTestId('pg-prev') as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByTestId('pg-next') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('pg-last') as HTMLButtonElement).disabled).toBe(true);
  });

  it('enables all controls in the middle', () => {
    render(Pagination, { props: { ...base, page: 2, pageCount: 5 } });
    for (const id of ['pg-first', 'pg-prev', 'pg-next', 'pg-last']) {
      expect((screen.getByTestId(id) as HTMLButtonElement).disabled).toBe(false);
    }
  });

  it('emits the correct target index for each control', async () => {
    const onPage = vi.fn();
    render(Pagination, { props: { ...base, page: 2, pageCount: 5, onPage } });
    await fireEvent.click(screen.getByTestId('pg-first'));
    expect(onPage).toHaveBeenLastCalledWith(0);
    await fireEvent.click(screen.getByTestId('pg-prev'));
    expect(onPage).toHaveBeenLastCalledWith(1);
    await fireEvent.click(screen.getByTestId('pg-next'));
    expect(onPage).toHaveBeenLastCalledWith(3);
    await fireEvent.click(screen.getByTestId('pg-last'));
    expect(onPage).toHaveBeenLastCalledWith(4); // pageCount - 1
  });

  it('treats a single page as both first and last (all nav disabled)', () => {
    render(Pagination, { props: { ...base, page: 0, pageCount: 1 } });
    for (const id of ['pg-first', 'pg-prev', 'pg-next', 'pg-last']) {
      expect((screen.getByTestId(id) as HTMLButtonElement).disabled).toBe(true);
    }
  });

  it('disables every control when disabled=true', () => {
    render(Pagination, { props: { ...base, page: 2, pageCount: 5, disabled: true } });
    for (const id of ['pg-first', 'pg-prev', 'pg-next', 'pg-last']) {
      expect((screen.getByTestId(id) as HTMLButtonElement).disabled).toBe(true);
    }
  });
});

// The four steps are icon-only at every width, each named by a full phrase it also says in a
// tooltip (DESIGN.md §5). Labelled steps did not fit the default 820 px window, and beside a
// Cyrillic label the |< and >| glyphs read as the letter «К» — so first and last draw « and ».
// happy-dom applies no Tailwind CSS, so what is pinned is the markup: the name, no visible text,
// no width-dependent class, the icon drawn, a tooltip that does not depend on the window's width,
// "N of M" and the page-size picker that never break inside, and a row that wraps rather than
// overflows where a box is narrower still.
describe('Pagination — icon-only steps', () => {
  const STEPS = [
    ['pg-first', 'First page', 'lucide-chevrons-left'],
    ['pg-prev', 'Previous page', 'lucide-chevron-left'],
    ['pg-next', 'Next page', 'lucide-chevron-right'],
    ['pg-last', 'Last page', 'lucide-chevrons-right'],
  ] as const;

  // A window `px` wide, as far as a `(min-width: …px)` or `(max-width: …px)` query can tell.
  const windowWidth = (px: number) =>
    vi.stubGlobal('matchMedia', (query: string) => {
      const q = /\((min|max)-width:\s*(\d+)px\)/.exec(query);
      return {
        matches: q !== null && (q[1] === 'min' ? px >= Number(q[2]) : px <= Number(q[2])),
        media: query,
        onchange: null,
        addEventListener() {},
        removeEventListener() {},
        addListener() {},
        removeListener() {},
        dispatchEvent: () => false,
      };
    });

  afterEach(() => {
    hideTooltip();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it('names each step by its full phrase and shows only its icon', () => {
    render(Pagination, { props: { ...base, page: 2 } });
    for (const [id, name, glyph] of STEPS) {
      const button = screen.getByTestId(id);
      expect(screen.getByRole('button', { name })).toBe(button);
      expect(button.textContent?.trim(), `${id} shows no text`).toBe('');
      const icons = [...button.querySelectorAll('svg')];
      expect(icons, `${id} draws one icon`).toHaveLength(1);
      expect(icons[0].classList.contains(glyph), `${id} draws ${glyph}`).toBe(true);
      // One look at every width: nothing in the step's markup depends on the window.
      expect(button.parentElement?.outerHTML, `${id}'s markup`).not.toMatch(/max-\[|sr-only/);
    }
  });

  it('says a step’s name in a tooltip in a wide window as in a narrow one', async () => {
    render(Pagination, { props: { ...base, page: 2 } });
    const next = screen.getByTestId('pg-next').parentElement as HTMLElement;

    for (const px of [820, 1400]) {
      windowWidth(px);
      revealTooltip(next);
      expect(tooltipState.visible, `${px} px`).toBe(true);
      expect(tooltipState.text, `${px} px`).toBe('Next page');
      // The name is the aria-label already: the tooltip mirrors it and describes nothing.
      expect(next.hasAttribute('aria-describedby')).toBe(false);
      await dismissTooltip(next);
    }
  });

  // A disabled button fires no pointer events: the tooltip sits on its wrapper (DESIGN.md §5).
  it('says it for a disabled step too', async () => {
    vi.useFakeTimers();
    render(Pagination, { props: { ...base, page: 0 } });
    const first = screen.getByTestId('pg-first');
    expect((first as HTMLButtonElement).disabled).toBe(true);
    windowWidth(1400);
    await fireEvent.mouseEnter(first.parentElement as HTMLElement);
    vi.advanceTimersByTime(OPEN_DELAY_MS);
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('First page');
  });

  it('keeps "N of M" and the page-size picker whole, and wraps the row instead of overflowing', () => {
    render(Pagination, { props: base });
    const label = screen.getByTestId('pg-label');
    expect(label.className).toMatch(/\bwhitespace-nowrap\b/);
    expect(label.parentElement?.className).toMatch(/\bflex-wrap\b/);
    const { container } = render(PageSizePicker);
    expect((container.firstElementChild as HTMLElement).className).toMatch(/\bwhitespace-nowrap\b/);
  });
});
