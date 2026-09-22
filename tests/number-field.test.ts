import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import NumberField from '$lib/ui/NumberField.svelte';

// The one rule under test: the field never accepts a value it would have to
// change. A refused entry is written back as the saved value and named; an
// accepted one is committed exactly once, and only when it differs.
function mount(
  over: Partial<{ value: number; min: number; max: number; hint: string; disabled: boolean }> = {},
) {
  const onCommit = vi.fn();
  render(NumberField, {
    props: {
      label: 'Keep newest logs (files)',
      value: 10,
      min: 1,
      onCommit,
      testId: 'nf',
      ...over,
    },
  });
  const input = screen.getByTestId('nf') as HTMLInputElement;
  return { onCommit, input };
}

async function type(input: HTMLInputElement, text: string) {
  input.value = text;
  await fireEvent.change(input);
}

const message = () => screen.getByRole('status').textContent?.trim() ?? '';

describe('NumberField', () => {
  it('commits a typed whole number within range once, with no message', async () => {
    const { onCommit, input } = mount();
    await type(input, '5');
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith(5);
    expect(message()).toBe('');
  });

  it('does not re-commit the saved value, and clears an earlier message', async () => {
    const { onCommit, input } = mount();
    await type(input, '');
    expect(message()).toContain('at least 1');
    await type(input, '10');
    expect(onCommit).not.toHaveBeenCalled();
    expect(message()).toBe('');
  });

  it.each([
    ['', 'blank'],
    ['-1', 'below the minimum'],
    ['2.7', 'not a whole number'],
  ])('refuses "%s" (%s): writes the saved value back, commits nothing, names the minimum', async (typed) => {
    const { onCommit, input } = mount();
    await type(input, typed);
    expect(input.value).toBe('10');
    expect(onCommit).not.toHaveBeenCalled();
    expect(message()).toBe('Enter a whole number of at least 1.');
  });

  it('names the range when a max is set', async () => {
    const { onCommit, input } = mount({ value: 11434, min: 1, max: 65535 });
    await type(input, '70000');
    expect(input.value).toBe('11434');
    expect(onCommit).not.toHaveBeenCalled();
    expect(message()).toBe('Enter a whole number from 1 to 65535.');
  });

  it('links the hint through aria-describedby', () => {
    const { input } = mount({ hint: 'At least 1.' });
    const id = input.getAttribute('aria-describedby');
    expect(id).toBeTruthy();
    expect(document.getElementById(id as string)?.textContent?.trim()).toBe('At least 1.');
  });

  it('disables the input and carries the disabled look', () => {
    const { input } = mount({ disabled: true });
    expect(input.disabled).toBe(true);
    expect(input.className).toContain('disabled:opacity-50');
  });
});
