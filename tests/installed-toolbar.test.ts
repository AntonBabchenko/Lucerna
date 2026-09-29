import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import InstalledToolbar from '$lib/mods/installed/InstalledToolbar.svelte';

const base = () => ({
  counts: {
    total: 3,
    enabled: 2,
    disabled: 1,
    updates: 1,
    issues: 2,
    needed: 0,
    unusedLibraries: 0,
  },
  filter: '',
  sortBy: 'name-asc' as const,
  viewFilter: 'all' as const,
  busy: false,
  checking: false,
  updateCount: 1,
  rechecking: false,
  onCheckUpdates: vi.fn(),
  onUpdateAll: vi.fn(),
  onRecheckAll: vi.fn(),
  onOpenModsFolder: vi.fn(),
});

describe('InstalledToolbar view filter (single mutually-exclusive group)', () => {
  it('renders All/Enabled/Disabled plus Updates/Issues as radios with counts', () => {
    render(InstalledToolbar, { props: base() });
    for (const name of [/All/, /Enabled/, /Disabled/, /Updates/, /Issues/]) {
      expect(screen.getByRole('radio', { name })).toBeTruthy();
    }
    // Default selection is All.
    expect(screen.getByRole('radio', { name: /All/ }).getAttribute('aria-checked')).toBe('true');
  });

  it('selecting a chip checks it and unchecks the others (mutually exclusive)', async () => {
    render(InstalledToolbar, { props: base() });
    await fireEvent.click(screen.getByRole('radio', { name: /Updates/ }));
    expect(screen.getByRole('radio', { name: /Updates/ }).getAttribute('aria-checked')).toBe(
      'true',
    );
    // All the others are now unchecked — no AND-combination.
    for (const name of [/All/, /Enabled/, /Disabled/, /Issues/]) {
      expect(screen.getByRole('radio', { name }).getAttribute('aria-checked')).toBe('false');
    }
  });

  it('exactly one radio is checked at any time', async () => {
    render(InstalledToolbar, { props: { ...base(), viewFilter: 'updates' } });
    await fireEvent.click(screen.getByRole('radio', { name: /Disabled/ }));
    const checked = screen
      .getAllByRole('radio')
      .filter((r) => r.getAttribute('aria-checked') === 'true');
    expect(checked).toHaveLength(1);
    expect(checked[0].textContent).toMatch(/Disabled/);
  });

  it('hides Updates/Issues radios when there are none', () => {
    render(InstalledToolbar, {
      props: {
        ...base(),
        counts: {
          total: 3,
          enabled: 3,
          disabled: 0,
          updates: 0,
          issues: 0,
          needed: 0,
          unusedLibraries: 0,
        },
      },
    });
    expect(screen.queryByRole('radio', { name: /Updates/ })).toBeNull();
    expect(screen.queryByRole('radio', { name: /Issues/ })).toBeNull();
    // The state filters remain.
    expect(screen.getByRole('radio', { name: /All/ })).toBeTruthy();
  });

  it('shows «Needed by others» and «Unused libraries» only when they have a count', () => {
    render(InstalledToolbar, {
      props: { ...base(), counts: { ...base().counts, needed: 2, unusedLibraries: 1 } },
    });
    expect(screen.getByRole('radio', { name: /Needed by others/ })).toBeTruthy();
    expect(screen.getByRole('radio', { name: /Unused libraries/ })).toBeTruthy();
  });

  it('has no separate Incompatible chip — incompatibility is part of Issues', () => {
    render(InstalledToolbar, { props: base() });
    expect(screen.queryByRole('radio', { name: /Incompatible/ })).toBeNull();
    expect(screen.queryByRole('radio', { name: /Needed by others/ })).toBeNull();
    expect(screen.queryByRole('radio', { name: /Unused libraries/ })).toBeNull();
  });
});

// Plan §5b V2 (screenshot 01f): Tab could land on a row under the sticky toolbar. The toolbar
// reserves its own height in the Add-ons scroll container (the rule: tests/sticky-edge.test.ts).
describe('InstalledToolbar keeps focus clear of itself', () => {
  it('reserves its height as its scroll container’s scroll-padding-top', () => {
    const scroller = document.createElement('div');
    scroller.style.overflowY = 'auto';
    document.body.append(scroller);
    const rect = vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: Element,
    ) {
      const h = (this as HTMLElement).dataset?.testid === 'installed-toolbar' ? 84 : 0;
      return { height: h, top: 0, bottom: h } as DOMRect;
    });
    try {
      render(InstalledToolbar, { props: base(), target: scroller });
      expect(scroller.style.scrollPaddingTop).toBe('92px');
    } finally {
      rect.mockRestore();
      scroller.remove();
    }
  });

  // Plan §5b V2 (screenshot 01e): rows scrolled under it were cut mid-glyph with no edge. Its edge
  // shows while it is stuck (`data-stuck`, tests/sticky-edge.test.ts) — and it is there, transparent,
  // at rest too, so it appearing moves nothing.
  it('shows a bottom edge while stuck, without changing its height', () => {
    render(InstalledToolbar, { props: base() });
    const bar = screen.getByTestId('installed-toolbar');
    expect(bar.classList).toContain('border-b');
    expect(bar.classList).toContain('border-transparent');
    expect(bar.classList).toContain('data-[stuck]:border-border-subtle');
  });
});

describe('InstalledToolbar issues tone', () => {
  it('is danger while something blocks, amber when only warnings remain', () => {
    const { unmount } = render(InstalledToolbar, {
      props: { ...base(), viewFilter: 'issues' as const },
    });
    expect(screen.getByTestId('installed-filter-issues').className).toContain('text-danger');
    unmount();
    render(InstalledToolbar, {
      props: { ...base(), viewFilter: 'issues' as const, issuesTone: 'warning' as const },
    });
    const chip = screen.getByTestId('installed-filter-issues');
    expect(chip.className).toContain('text-warning-text');
    expect(chip.className).not.toContain('text-danger');
  });

  // Plan §5b V2: at rest the chip was plain grey, with the amber triangle, while mods stopped the
  // game. Now it keeps red on its icon and count while anything blocks — attention, not selection:
  // no fill, no border, so it never looks chosen — and red takes the ✕; the triangle is amber's.
  it('at rest it stays red on its ✕ and its count while something blocks', () => {
    render(InstalledToolbar, { props: base() });
    const chip = screen.getByTestId('installed-filter-issues');
    expect(chip.getAttribute('aria-checked')).toBe('false');
    expect(chip.className).not.toContain('bg-danger');
    expect(chip.className).not.toContain('border-danger');
    const icon = chip.querySelector('svg');
    expect(icon?.classList).toContain('lucide-circle-x');
    expect(icon?.classList).toContain('text-danger');
    expect(within(chip).getByText('2').className).toContain('text-danger');
  });

  it('with only warnings left it rests neutral, with the amber triangle', () => {
    render(InstalledToolbar, { props: { ...base(), issuesTone: 'warning' as const } });
    const chip = screen.getByTestId('installed-filter-issues');
    const icon = chip.querySelector('svg');
    expect(icon?.classList).toContain('lucide-triangle-alert');
    expect(chip.innerHTML).not.toContain('text-warning-text');
    expect(chip.innerHTML).not.toContain('text-danger');
  });
});
