import { render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import CountPill, { countPillClass } from '$lib/ui/cards/CountPill.svelte';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import { hoverTooltip } from '../../test-utils/hover-tooltip';

function text(label: string) {
  return createRawSnippet(() => ({ render: () => `<span>${label}</span>` }));
}

afterEach(() => hideTooltip());

describe('CountPill', () => {
  it('renders the count', () => {
    render(CountPill, { props: { children: text('3'), testid: 'p' } });
    expect(screen.getByTestId('p').textContent).toContain('3');
  });

  it('owns the size scale as named tokens, not per-call-site literals', () => {
    expect(countPillClass('sm')).toContain('h-[15px]');
    expect(countPillClass('sm')).toContain('min-w-[15px]');
    expect(countPillClass('md')).toContain('h-[18px]');
    expect(countPillClass('md')).toContain('min-w-[18px]');
  });

  it('every size shares one fill, one radius and one type ramp', () => {
    // The drift this primitive exists to stop: three copies, two box sizes, and
    // one of them missing leading-none. Size may vary; nothing else may.
    for (const cls of [countPillClass('sm'), countPillClass('md')]) {
      expect(cls).toContain('bg-success');
      expect(cls).toContain('rounded-full');
      expect(cls).toContain('text-[10px]');
      expect(cls).toContain('font-semibold');
      expect(cls).toContain('leading-none');
      expect(cls).toContain('text-white');
    }
  });

  it('appends the caller class without letting it replace the recipe', () => {
    render(CountPill, { props: { children: text('3'), testid: 'p', class: 'ml-1' } });
    const pill = screen.getByTestId('p');
    expect(pill.className).toContain('ml-1');
    expect(pill.className).toContain('rounded-full');
    expect(pill.className).toContain('bg-success');
  });

  it('routes `title` through the tooltip layer, never a native title=', () => {
    render(CountPill, { props: { children: text('3'), testid: 'p', title: '3 updates' } });
    const pill = screen.getByTestId('p');
    expect(pill.getAttribute('title')).toBeNull();
    hoverTooltip(pill);
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('3 updates');
  });
});
