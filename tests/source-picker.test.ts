import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { ModSource } from '$lib/ipc/bindings';
import SourcePicker from '$lib/mods/SourcePicker.svelte';

describe('SourcePicker', () => {
  it('renders both source options when opened', async () => {
    render(SourcePicker, { props: { value: 'modrinth', onChange: () => {} } });
    await fireEvent.click(screen.getByLabelText('Mod source'));
    const labels = screen.getAllByRole('option').map((o) => o.textContent?.trim());
    expect(labels).toEqual(['Modrinth', 'CurseForge']);
  });

  it('shows the current value and fires onChange when another source is picked', async () => {
    let current: ModSource = 'modrinth';
    render(SourcePicker, {
      props: { value: current, onChange: (v: ModSource) => (current = v) },
    });
    const trigger = screen.getByLabelText('Mod source');
    expect(trigger.textContent).toContain('Modrinth');
    await fireEvent.click(trigger);
    await fireEvent.mouseDown(screen.getByRole('option', { name: 'CurseForge' }));
    expect(current).toBe('curseforge');
  });
});
