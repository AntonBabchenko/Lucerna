import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import SourcePicker from '$lib/mods/SourcePicker.svelte';

describe('SourcePicker', () => {
  it('renders both source options', () => {
    render(SourcePicker, { props: { value: 'modrinth' } });
    const select = screen.getByLabelText('Mod source') as HTMLSelectElement;
    expect(select).toBeTruthy();
    const options = Array.from(select.options).map((o) => o.value);
    expect(options).toEqual(['modrinth', 'curseforge']);
  });

  it('reflects the bound value and updates when the user selects another source', async () => {
    render(SourcePicker, { props: { value: 'modrinth' } });
    const select = screen.getByLabelText('Mod source') as HTMLSelectElement;
    expect(select.value).toBe('modrinth');
    await fireEvent.change(select, { target: { value: 'curseforge' } });
    expect(select.value).toBe('curseforge');
  });
});
