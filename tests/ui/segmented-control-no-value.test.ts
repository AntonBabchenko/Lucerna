// A settings control that has no value yet (still loading, or unreadable) shows
// none and can't be used; one whose value is simply unknown still has a tab
// stop and arrow keys that start from the first option.
import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import SegmentedControl from '$lib/ui/SegmentedControl.svelte';

const OPTIONS = [
  { value: 'keep', label: 'Keep open' },
  { value: 'minimise', label: 'Minimise' },
  { value: 'hide_to_tray', label: 'Hide to tray' },
];
const button = (name: string) => screen.getByRole('button', { name }) as HTMLButtonElement;

function mount(value: string | null, disabled = false) {
  const onChange = vi.fn();
  render(SegmentedControl, {
    options: OPTIONS,
    value,
    onChange,
    variant: 'boxed',
    ariaLabel: 'When a game starts',
    dataTestid: 'group',
    disabled,
  });
  return onChange;
}

describe('a segmented control with no value', () => {
  it('claims no option, and its first option holds the tab stop', () => {
    mount(null);
    for (const o of OPTIONS) expect(button(o.label).getAttribute('aria-pressed')).toBe('false');
    expect(button('Keep open').tabIndex).toBe(0);
    expect(button('Minimise').tabIndex).toBe(-1);
  });

  it('starts the arrow keys from the first option', async () => {
    const onChange = mount(null);
    await fireEvent.keyDown(screen.getByTestId('group'), { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('minimise');
    await fireEvent.keyDown(screen.getByTestId('group'), { key: 'ArrowLeft' });
    expect(onChange).toHaveBeenLastCalledWith('hide_to_tray');
    await fireEvent.keyDown(screen.getByTestId('group'), { key: 'Home' });
    expect(onChange).toHaveBeenLastCalledWith('keep');
  });
});

describe('a disabled segmented control', () => {
  it('disables every option and ignores clicks and arrows', async () => {
    const onChange = mount('minimise', true);
    for (const o of OPTIONS) expect(button(o.label).disabled).toBe(true);
    await fireEvent.click(button('Hide to tray'));
    await fireEvent.keyDown(screen.getByTestId('group'), { key: 'ArrowRight' });
    expect(onChange).not.toHaveBeenCalled();
  });
});
