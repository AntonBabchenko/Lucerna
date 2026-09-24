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

  // A chosen option is a state, not an action. The CTA fill (.btn-primary) made
  // the pressed segment read as the button to press next to two bare labels;
  // the boxed variant is a recessed track with a neutral thumb and a small
  // accent mark instead (DESIGN.md §6, SegmentedControl).
  it('boxed segments carry no btn-* purpose class', () => {
    setup({ value: 'grid' });
    for (const id of ['layout-grid', 'layout-list']) {
      expect(screen.getByTestId(id).className).not.toMatch(/(^|\s)btn-/);
    }
  });

  it('the pressed segment is the thumb with one accent mark; the others are plain', () => {
    setup({ value: 'grid' });
    const active = screen.getByTestId('layout-grid');
    const inactive = screen.getByTestId('layout-list');
    expect(active.classList.contains('bg-control-thumb')).toBe(true);
    const marks = active.querySelectorAll('[data-segment-mark]');
    expect(marks).toHaveLength(1);
    expect(marks[0].getAttribute('aria-hidden')).toBe('true');
    expect(inactive.classList.contains('bg-control-thumb')).toBe(false);
    expect(inactive.querySelector('[data-segment-mark]')).toBeNull();
    expect(inactive.classList.contains('text-secondary')).toBe(true);
  });

  it('with no value nothing is drawn as chosen', () => {
    setup({ value: null });
    expect(document.querySelector('[data-segment-mark]')).toBeNull();
    expect(document.querySelector('.bg-control-thumb')).toBeNull();
  });

  it('the track hugs its segments and does not clip the focus ring', () => {
    setup();
    const group = screen.getByRole('group', { name: 'Layout' });
    // w-fit: inside a flex column an inline-flex root is stretched to the
    // column's width, drawing an empty track tail past the last segment.
    expect(group.classList.contains('w-fit')).toBe(true);
    expect(group.classList.contains('bg-control-track')).toBe(true);
    // overflow-hidden cut the 2 px outline-offset focus ring to a sliver.
    expect(group.classList.contains('overflow-hidden')).toBe(false);
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
