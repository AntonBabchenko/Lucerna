import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// mcVersions rune is read by the combobox; a couple of releases is enough.
vi.mock('$lib/settings/state.svelte', () => ({
  mcVersions: {
    value: [
      { id: '1.20.1', version_type: 'release' },
      { id: '1.21', version_type: 'release' },
    ],
  },
}));

import McVersionCombobox from '$lib/mods/McVersionCombobox.svelte';

describe('McVersionCombobox', () => {
  it('opens the listbox on focus and commits a pick when enabled', async () => {
    render(McVersionCombobox, { props: { dataTestid: 'mc', value: '' } });
    const input = screen.getByTestId('mc') as HTMLInputElement;
    expect(input.disabled).toBe(false);
    await fireEvent.focus(input);
    expect(screen.getByRole('listbox')).toBeTruthy();
  });

  it('is inert when disabled — input is disabled and focus does not open the dropdown', async () => {
    render(McVersionCombobox, { props: { dataTestid: 'mc', value: '', disabled: true } });
    const input = screen.getByTestId('mc') as HTMLInputElement;
    expect(input.disabled).toBe(true);
    // Even if a focus event is dispatched, the handler is guarded → no listbox.
    await fireEvent.focus(input);
    expect(screen.queryByRole('listbox')).toBeNull();
    // And typing does not open it either.
    await fireEvent.input(input);
    expect(screen.queryByRole('listbox')).toBeNull();
  });
});
