import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import SegmentedControl from '$lib/ui/SegmentedControl.svelte';

const BOXED = [
  { value: 'grid', icon: 'grid' as const, testId: 'layout-grid' },
  { value: 'list', icon: 'list' as const, testId: 'layout-list' },
];

function setup(props: Record<string, unknown> = {}) {
  const onChange = vi.fn();
  render(SegmentedControl, {
    props: {
      options: BOXED,
      value: 'grid',
      onChange,
      variant: 'boxed',
      ariaLabel: 'Layout',
      ...props,
    },
  });
  return { onChange };
}

describe('SegmentedControl', () => {
  it('renders a labelled group with one button per option', () => {
    setup();
    const group = screen.getByRole('group', { name: 'Layout' });
    expect(group).toBeTruthy();
    expect(screen.getAllByRole('button')).toHaveLength(2);
  });

  it('marks the active option with aria-pressed and forwards testId', () => {
    setup({ value: 'list' });
    expect(screen.getByTestId('layout-grid').getAttribute('aria-pressed')).toBe('false');
    expect(screen.getByTestId('layout-list').getAttribute('aria-pressed')).toBe('true');
  });

  it('fires onChange with the clicked option value', async () => {
    const { onChange } = setup();
    await fireEvent.click(screen.getByTestId('layout-list'));
    expect(onChange).toHaveBeenCalledWith('list');
  });

  it('roving ArrowRight/ArrowLeft selects the next/previous option (boxed)', async () => {
    const { onChange } = setup({ value: 'grid' });
    const group = screen.getByRole('group', { name: 'Layout' });
    await fireEvent.keyDown(group, { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('list');
    onChange.mockClear();
    await fireEvent.keyDown(group, { key: 'ArrowLeft' });
    expect(onChange).toHaveBeenLastCalledWith('list'); // wraps from grid back to list
  });

  it('Home and End jump to first and last option', async () => {
    const { onChange } = setup({ value: 'list' });
    const group = screen.getByRole('group', { name: 'Layout' });
    await fireEvent.keyDown(group, { key: 'Home' });
    expect(onChange).toHaveBeenLastCalledWith('grid');
    await fireEvent.keyDown(group, { key: 'End' });
    expect(onChange).toHaveBeenLastCalledWith('list');
  });

  it('boxed active option is btn-primary — never stacked with another btn-* purpose class', () => {
    setup({ value: 'grid' });
    const active = screen.getByTestId('layout-grid');
    expect(active).toHaveBtnVariant('primary');
    // Two btn-* purpose classes must never be stacked on one element: at equal
    // specificity the later app.css rule (.btn-secondary / .btn-ghost) wins the
    // cascade and kills the active fill.
    expect(active).not.toHaveBtnVariant('secondary');
    expect(active).not.toHaveBtnVariant('ghost');
    expect(active).toHaveBtnSize('sm');
  });

  it('boxed inactive option is btn-ghost, not btn-primary or btn-secondary', () => {
    setup({ value: 'grid' });
    const inactive = screen.getByTestId('layout-list');
    expect(inactive).toHaveBtnVariant('ghost');
    expect(inactive).not.toHaveBtnVariant('primary');
    expect(inactive).not.toHaveBtnVariant('secondary');
    expect(inactive).toHaveBtnSize('sm');
  });

  it('renders text labels and roving works in the inline variant', async () => {
    const onChange = vi.fn();
    render(SegmentedControl, {
      props: {
        options: [
          { value: '20', label: '20', testId: 'page-size-20' },
          { value: '50', label: '50', testId: 'page-size-50' },
        ],
        value: '20',
        onChange,
        variant: 'inline',
        ariaLabel: 'Per page',
      },
    });
    expect(screen.getByTestId('page-size-50').textContent).toContain('50');
    const group = screen.getByRole('group', { name: 'Per page' });
    await fireEvent.keyDown(group, { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('50');
  });
});
