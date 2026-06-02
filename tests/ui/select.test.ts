import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import Select from '$lib/ui/Select.svelte';

const OPTIONS = [
  { value: 'a', label: 'Apple' },
  { value: 'b', label: 'Banana' },
  { value: 'c', label: 'Cherry', disabled: true },
  { value: 'd', label: 'Date' },
];

function setup(props: Record<string, unknown> = {}) {
  const onChange = vi.fn();
  render(Select, {
    props: { value: null, options: OPTIONS, onChange, placeholder: 'Pick one', ...props },
  });
  const trigger = screen.getByRole('combobox');
  return { onChange, trigger };
}

describe('Select', () => {
  it('shows the greyed placeholder when value is null', () => {
    const { trigger } = setup();
    expect(trigger.textContent).toContain('Pick one');
  });

  it('shows the selected option label when value is set', () => {
    const { trigger } = setup({ value: 'b' });
    expect(trigger.textContent).toContain('Banana');
  });

  it('opens the listbox on click and renders all options', async () => {
    const { trigger } = setup();
    await fireEvent.click(trigger);
    expect(screen.getByRole('listbox')).toBeTruthy();
    expect(screen.getAllByRole('option')).toHaveLength(4);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
  });

  it('opens on ArrowDown and moves active past disabled options', async () => {
    const { trigger } = setup({ value: null });
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    const date = screen.getByRole('option', { name: 'Date' });
    expect(trigger.getAttribute('aria-activedescendant')).toBe(date.id);
  });

  it('selects the active option with Enter, fires onChange, and closes', async () => {
    const { trigger, onChange } = setup({ value: null });
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    await fireEvent.keyDown(trigger, { key: 'Enter' });
    expect(onChange).toHaveBeenCalledWith('b');
    expect(screen.queryByRole('listbox')).toBeNull();
  });

  it('commits on option click', async () => {
    const { trigger, onChange } = setup();
    await fireEvent.click(trigger);
    await fireEvent.mouseDown(screen.getByRole('option', { name: 'Date' }));
    expect(onChange).toHaveBeenCalledWith('d');
  });

  it('Home and End jump to first and last enabled options', async () => {
    const { trigger } = setup();
    await fireEvent.click(trigger);
    await fireEvent.keyDown(trigger, { key: 'End' });
    expect(trigger.getAttribute('aria-activedescendant')).toBe(
      screen.getByRole('option', { name: 'Date' }).id,
    );
    await fireEvent.keyDown(trigger, { key: 'Home' });
    expect(trigger.getAttribute('aria-activedescendant')).toBe(
      screen.getByRole('option', { name: 'Apple' }).id,
    );
  });

  it('Escape closes without firing onChange', async () => {
    const { trigger, onChange } = setup();
    await fireEvent.click(trigger);
    await fireEvent.keyDown(trigger, { key: 'Escape' });
    expect(screen.queryByRole('listbox')).toBeNull();
    expect(onChange).not.toHaveBeenCalled();
  });

  it('typeahead while closed commits the first label match', async () => {
    const { trigger, onChange } = setup();
    await fireEvent.keyDown(trigger, { key: 'd' });
    expect(onChange).toHaveBeenCalledWith('d');
  });

  it('does not open when disabled', async () => {
    const { trigger } = setup({ disabled: true });
    await fireEvent.click(trigger);
    expect(screen.queryByRole('listbox')).toBeNull();
  });

  it('does not commit a disabled option on click', async () => {
    const { trigger, onChange } = setup();
    await fireEvent.click(trigger);
    await fireEvent.mouseDown(screen.getByRole('option', { name: 'Cherry' }));
    expect(onChange).not.toHaveBeenCalled();
  });

  it('marks the selected option with aria-selected', async () => {
    const { trigger } = setup({ value: 'b' });
    await fireEvent.click(trigger);
    expect(screen.getByRole('option', { name: 'Banana' }).getAttribute('aria-selected')).toBe(
      'true',
    );
  });

  it('exposes ariaLabel, id, and data-testid on the trigger', () => {
    setup({ ariaLabel: 'Fruit', id: 'fruit', dataTestid: 'fruit-select' });
    const trigger = screen.getByTestId('fruit-select');
    expect(trigger.getAttribute('aria-label')).toBe('Fruit');
    expect(trigger.id).toBe('fruit');
  });

  it('click-outside closes the listbox', async () => {
    const { trigger } = setup();
    await fireEvent.click(trigger);
    await fireEvent.mouseDown(document.body);
    expect(screen.queryByRole('listbox')).toBeNull();
  });
});
