import { fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import Select from '$lib/ui/Select.svelte';

// Labels carry a suffix the way the sidebar's do («Name · Loader version»); the filter reads
// only the name through `filter.text`.
const NAMES = ['Survival', 'Skyblock', 'Creative', 'Fabric test', 'Hardcore'];
const OPTIONS = NAMES.map((n, i) => ({ value: `i${i}`, label: `${n} · Forge 1.20.1` }));

const FILTER = {
  placeholder: 'Filter instances…',
  ariaLabel: 'Filter instances by name',
  noMatches: (q: string) => `No instance name contains “${q}”.`,
  text: (o: { label: string }) => o.label.split(' · ')[0],
};

function setup(props: Record<string, unknown> = {}) {
  const onChange = vi.fn();
  const onOptionContextMenu = vi.fn();
  render(Select, {
    props: {
      value: 'i0',
      options: OPTIONS,
      onChange,
      onOptionContextMenu,
      filter: FILTER,
      ...props,
    },
  });
  const trigger = screen.getByRole('combobox');
  return { onChange, onOptionContextMenu, trigger };
}

const field = () =>
  screen.getByRole('combobox', { name: 'Filter instances by name' }) as HTMLInputElement;
const rows = () => screen.queryAllByRole('option').map((o) => o.textContent?.trim());

async function type(text: string) {
  await fireEvent.input(field(), { target: { value: text } });
}

describe('Select — a filter field in the open list', () => {
  it('opens with an empty field that holds the focus, every row shown', async () => {
    const { trigger } = setup();
    await fireEvent.click(trigger);
    await tick();
    expect(field().value).toBe('');
    expect(document.activeElement).toBe(field());
    expect(rows()).toHaveLength(5);
  });

  it('narrows the rows to names containing the text, ignoring case — not the loader suffix', async () => {
    const { trigger } = setup();
    await fireEvent.click(trigger);
    await type('  SK ');
    expect(rows()).toEqual(['Skyblock · Forge 1.20.1']);
    await type('forge');
    expect(rows()).toEqual([]);
    await type('fab');
    expect(rows()).toEqual(['Fabric test · Forge 1.20.1']);
  });

  it('Enter takes the top match; ArrowDown then Enter takes the next one', async () => {
    const { trigger, onChange } = setup();
    await fireEvent.click(trigger);
    await type('o'); // Skyblock, Hardcore
    await fireEvent.keyDown(field(), { key: 'Enter' });
    expect(onChange).toHaveBeenLastCalledWith('i1');
    expect(screen.queryByRole('listbox')).toBeNull();
    expect(document.activeElement).toBe(trigger);

    await fireEvent.click(trigger);
    await type('o');
    await fireEvent.keyDown(field(), { key: 'ArrowDown' });
    await fireEvent.keyDown(field(), { key: 'Enter' });
    expect(onChange).toHaveBeenLastCalledWith('i4');
  });

  it('a letter on the closed trigger opens the list with that letter in the field and picks nothing', async () => {
    const { trigger, onChange } = setup();
    trigger.focus();
    await fireEvent.keyDown(trigger, { key: 'h' });
    await tick();
    expect(field().value).toBe('h');
    expect(document.activeElement).toBe(field());
    expect(rows()).toEqual(['Hardcore · Forge 1.20.1']);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('says honestly that no name matches, shows no rows, and Enter picks nothing', async () => {
    const { trigger, onChange } = setup();
    await fireEvent.click(trigger);
    await type('zzz');
    expect(rows()).toEqual([]);
    expect(screen.getByRole('status').textContent).toContain('No instance name contains “zzz”.');
    await fireEvent.keyDown(field(), { key: 'Enter' });
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole('listbox')).toBeTruthy();
  });

  it('Escape closes the list, gives the focus back to the trigger, and the next open starts empty', async () => {
    const { trigger, onChange } = setup();
    await fireEvent.click(trigger);
    await type('sky');
    await fireEvent.keyDown(field(), { key: 'Escape' });
    expect(screen.queryByRole('listbox')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(onChange).not.toHaveBeenCalled();
    await fireEvent.click(trigger);
    expect(field().value).toBe('');
    expect(rows()).toHaveLength(5);
  });

  it('Shift+F10 in the field opens the active row’s menu', async () => {
    const { trigger, onOptionContextMenu } = setup();
    await fireEvent.click(trigger);
    await type('hard');
    await fireEvent.keyDown(field(), { key: 'F10', shiftKey: true });
    expect(onOptionContextMenu.mock.calls[0][0].value).toBe('i4');
    expect(onOptionContextMenu.mock.calls[0][2]).toEqual({ byKeyboard: true });
  });
});

describe('Select — without a filter nothing changes', () => {
  it('has no field and a letter on the closed trigger still picks the first match', async () => {
    const { trigger, onChange } = setup({ filter: undefined });
    await fireEvent.click(trigger);
    expect(screen.queryByRole('combobox', { name: 'Filter instances by name' })).toBeNull();
    await fireEvent.keyDown(trigger, { key: 'Escape' });
    trigger.focus();
    await fireEvent.keyDown(trigger, { key: 'h' });
    expect(onChange).toHaveBeenCalledWith('i4');
  });
});
