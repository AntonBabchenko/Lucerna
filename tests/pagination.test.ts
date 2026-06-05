import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import Pagination from '$lib/ui/Pagination.svelte';

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
