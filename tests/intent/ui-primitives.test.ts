// ui-primitives group intent coverage.
//
// tests/close-button.test.ts (cluster D predecessor) covers rendering,
// aria-label, and raw className string checks. This file adds:
//   — toHaveBtnVariant matcher assertion (the canonical intent check)
//   — extra class prop forwarding verified via toHaveBtnVariant + className
//   — onClick fires exactly once
//   — no-throw guard when onClick is undefined

import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import CloseButton from '$lib/ui/CloseButton.svelte';

describe('CloseButton — full intent coverage', () => {
  it('default aria-label is "Close"', () => {
    render(CloseButton, { props: {} });
    const btn = screen.getByRole('button');
    expect(btn.getAttribute('aria-label')).toBe('Close');
  });

  it('renders icon content', () => {
    render(CloseButton, { props: {} });
    const btn = screen.getByRole('button');
    expect(btn.querySelector('svg')).toBeTruthy();
  });

  it('carries btn-icon variant via toHaveBtnVariant matcher', () => {
    render(CloseButton, { props: {} });
    const btn = screen.getByRole('button') as HTMLElement;
    expect(btn).toHaveBtnVariant('icon');
  });

  it('extra class prop appended, btn-icon preserved', () => {
    render(CloseButton, { props: { class: 'absolute top-1 right-1' } });
    const btn = screen.getByRole('button') as HTMLElement;
    expect(btn).toHaveBtnVariant('icon');
    expect(btn.className).toContain('absolute');
    expect(btn.className).toContain('top-1');
    expect(btn.className).toContain('right-1');
  });

  it('onClick fires exactly once when clicked', async () => {
    const onClick = vi.fn();
    render(CloseButton, { props: { onClick } });
    const btn = screen.getByRole('button');
    await fireEvent.click(btn);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('does not throw when onClick is undefined and clicked', async () => {
    render(CloseButton, { props: {} });
    const btn = screen.getByRole('button');
    await fireEvent.click(btn);
    expect(btn).toBeDefined();
  });
});
