import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import SkinLibraryEntryDialog from '$lib/accounts/SkinLibraryEntryDialog.svelte';

const base = {
  title: 'Save skin to library',
  initial: { name: 'My skin', variant: 'classic' as const, capeId: null },
  onCancel: () => {},
  onSubmit: () => {},
};

describe('SkinLibraryEntryDialog', () => {
  it('renders the title and the prefilled name', () => {
    render(SkinLibraryEntryDialog, { props: { ...base } });
    expect(screen.getByText('Save skin to library')).toBeTruthy();
    expect((screen.getByLabelText(/name/i) as HTMLInputElement).value).toBe('My skin');
  });

  it('disables submit for a blank name and blocks form submit', async () => {
    const onSubmit = vi.fn();
    render(SkinLibraryEntryDialog, { props: { ...base, onSubmit } });
    const input = screen.getByLabelText(/name/i) as HTMLInputElement;

    await fireEvent.input(input, { target: { value: '   ' } });
    expect((screen.getByRole('button', { name: /^save$/i }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    const form = input.closest('form');
    if (!form) throw new Error('name input is not inside a form');
    await fireEvent.submit(form);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('submits the trimmed name with variant and cape id', async () => {
    const onSubmit = vi.fn();
    render(SkinLibraryEntryDialog, {
      props: {
        ...base,
        initial: { name: 'old', variant: 'slim' as const, capeId: 'c9' },
        capes: [{ id: 'c9', alias: 'Migrator', texture_url: 'https://t.example/c9', is_active: false }],
        onSubmit,
      },
    });
    await fireEvent.input(screen.getByLabelText(/name/i), { target: { value: '  Wizard  ' } });
    await fireEvent.click(screen.getByRole('button', { name: /^save$/i }));
    expect(onSubmit).toHaveBeenCalledExactlyOnceWith({
      name: 'Wizard',
      variant: 'slim',
      capeId: 'c9',
    });
  });

  it('hides the cape row when the surface has no capes', () => {
    render(SkinLibraryEntryDialog, { props: { ...base } });
    expect(screen.queryByText(/^cape$/i)).toBeNull();
  });

  it('shows the cape row when capes are provided', () => {
    render(SkinLibraryEntryDialog, {
      props: {
        ...base,
        capes: [{ id: 'c1', alias: 'Vanilla', texture_url: 'https://t.example/c1', is_active: true }],
      },
    });
    expect(screen.getByText(/^cape$/i)).toBeTruthy();
    // Collapsed select shows the "no cape" default from initial.capeId = null.
    expect(screen.getByText(/no cape/i)).toBeTruthy();
  });

  it('renders a backend error as an alert and keeps the dialog open', () => {
    render(SkinLibraryEntryDialog, {
      props: { ...base, error: "That didn't go through. Try again." },
    });
    expect(screen.getByRole('alert').textContent).toMatch(/didn't go through/i);
  });

  it('fires onCancel on Escape', async () => {
    const onCancel = vi.fn();
    render(SkinLibraryEntryDialog, { props: { ...base, onCancel } });
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
