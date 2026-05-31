import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import SegmentedControl from '$lib/browse/SegmentedControl.svelte';

const OPTIONS = [
  { value: '', label: 'Any' },
  { value: 'fabric', label: 'Fabric' },
  { value: 'forge', label: 'Forge' },
];

describe('SegmentedControl', () => {
  it('renders a radiogroup with the given aria-label and one radio per option', () => {
    render(SegmentedControl, {
      props: { value: '', options: OPTIONS, ariaLabel: 'Loader filter', onChange: () => {} },
    });
    const group = screen.getByRole('radiogroup', { name: /loader filter/i });
    expect(group).toBeTruthy();
    expect(screen.getAllByRole('radio')).toHaveLength(3);
  });

  it('marks the selected option aria-checked and gives it the active classes', () => {
    render(SegmentedControl, {
      props: { value: 'fabric', options: OPTIONS, ariaLabel: 'Loader filter', onChange: () => {} },
    });
    const fabric = screen.getByRole('radio', { name: 'Fabric' });
    expect(fabric.getAttribute('aria-checked')).toBe('true');
    expect(fabric.className).toContain('bg-accent/15');
    expect(fabric.className).toContain('text-accent');
  });

  it('roving tabindex: only the selected radio is tabbable', () => {
    render(SegmentedControl, {
      props: { value: 'fabric', options: OPTIONS, ariaLabel: 'Loader filter', onChange: () => {} },
    });
    expect(screen.getByRole('radio', { name: 'Fabric' }).getAttribute('tabindex')).toBe('0');
    expect(screen.getByRole('radio', { name: 'Any' }).getAttribute('tabindex')).toBe('-1');
  });

  it('calls onChange with the option value when clicked', async () => {
    const onChange = vi.fn();
    render(SegmentedControl, {
      props: { value: '', options: OPTIONS, ariaLabel: 'Loader filter', onChange },
    });
    await fireEvent.click(screen.getByRole('radio', { name: 'Forge' }));
    expect(onChange).toHaveBeenCalledWith('forge');
  });

  it('ArrowRight selects the next option via onChange', async () => {
    const onChange = vi.fn();
    render(SegmentedControl, {
      props: { value: 'fabric', options: OPTIONS, ariaLabel: 'Loader filter', onChange },
    });
    await fireEvent.keyDown(screen.getByRole('radiogroup'), { key: 'ArrowRight' });
    expect(onChange).toHaveBeenCalledWith('forge');
  });
});
