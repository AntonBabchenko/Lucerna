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
  it('renders First, Prev, Next, Last and a page label', () => {
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

// Plan §5e: at the launcher's default 820 px the labelled row was 654 px in a 556 px box, and the
// page-size picker sat off the window. Below 1100 px the four steps show their icons only. happy-dom
// applies no Tailwind CSS, so what is pinned is the markup that carries the rule: a label hidden
// only under `max-[1099px]:` (Tailwind's `max-[Npx]` includes N, so 1100 px and wider are
// unchanged) and still the step's name, a tooltip that says it while it is hidden, "N of M" and the
// page-size picker that never break inside, and a row that wraps rather than overflows where a box
// is narrower still.
describe('Pagination — in a narrow window', () => {
  const STEPS = [
    ['pg-first', 'First'],
    ['pg-prev', 'Prev'],
    ['pg-next', 'Next'],
    ['pg-last', 'Last'],
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

  it('hides a step’s label only below 1100 px, and it stays the step’s name', () => {
    render(Pagination, { props: { ...base, page: 2 } });
    for (const [id, label] of STEPS) {
      const button = screen.getByTestId(id);
      expect(screen.getByRole('button', { name: label })).toBe(button);
      const text = [...button.querySelectorAll('span')].find(
        (s) => s.textContent?.trim() === label,
      );
      expect(text?.className.split(/\s+/), `${id}'s label`).toEqual(['max-[1099px]:sr-only']);
    }
  });

  it('says a step’s label in a tooltip while it is hidden, never beside a shown one', async () => {
    render(Pagination, { props: { ...base, page: 2 } });
    const next = screen.getByTestId('pg-next').parentElement as HTMLElement;

    windowWidth(820);
    revealTooltip(next);
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Next');
    // The name is the label already: the tooltip mirrors it and describes nothing.
    expect(next.hasAttribute('aria-describedby')).toBe(false);
    await dismissTooltip(next);

    // 1100 px is a wide window: the label shows, and so no tooltip repeats it.
    windowWidth(1100);
    revealTooltip(next);
    expect(tooltipState.visible).toBe(false);
  });

  // A disabled button fires no pointer events: the tooltip sits on its wrapper (DESIGN.md §5).
  it('says it for a disabled step too', async () => {
    vi.useFakeTimers();
    render(Pagination, { props: { ...base, page: 0 } });
    const first = screen.getByTestId('pg-first');
    expect((first as HTMLButtonElement).disabled).toBe(true);
    windowWidth(820);
    await fireEvent.mouseEnter(first.parentElement as HTMLElement);
    vi.advanceTimersByTime(OPEN_DELAY_MS);
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('First');
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
