import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import AddOfflineAccountDialog from '$lib/accounts/AddOfflineAccountDialog.svelte';

describe('AddOfflineAccountDialog', () => {
  it('shows a heading, an explanation, and a labelled name field', () => {
    render(AddOfflineAccountDialog, {
      props: { error: null, onCancel: vi.fn(), onSubmit: vi.fn() },
    });
    expect(screen.getByText(/add an offline account/i)).toBeTruthy();
    // The discoverability fix: an explicit label, not just a placeholder.
    expect(screen.getByLabelText(/player name/i)).toBeTruthy();
  });

  it('disables the confirm button until a non-blank name is entered', async () => {
    render(AddOfflineAccountDialog, {
      props: { error: null, onCancel: vi.fn(), onSubmit: vi.fn() },
    });
    const confirm = screen.getByRole('button', { name: /add account/i }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);

    await fireEvent.input(screen.getByLabelText(/player name/i), { target: { value: '   ' } });
    expect(confirm.disabled).toBe(true);

    await fireEvent.input(screen.getByLabelText(/player name/i), { target: { value: 'Steve' } });
    expect(confirm.disabled).toBe(false);
  });

  it('submits the trimmed name from the confirm button', async () => {
    const onSubmit = vi.fn();
    render(AddOfflineAccountDialog, { props: { error: null, onCancel: vi.fn(), onSubmit } });

    await fireEvent.input(screen.getByLabelText(/player name/i), {
      target: { value: '  Steve  ' },
    });
    await fireEvent.click(screen.getByRole('button', { name: /add account/i }));

    expect(onSubmit).toHaveBeenCalledExactlyOnceWith('Steve');
  });

  it('submits when the form is submitted (Enter)', async () => {
    const onSubmit = vi.fn();
    render(AddOfflineAccountDialog, { props: { error: null, onCancel: vi.fn(), onSubmit } });

    const input = screen.getByLabelText(/player name/i) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'Alex' } });
    await fireEvent.submit(input.closest('form')!);

    expect(onSubmit).toHaveBeenCalledExactlyOnceWith('Alex');
  });

  it('does not submit a blank name', async () => {
    const onSubmit = vi.fn();
    render(AddOfflineAccountDialog, { props: { error: null, onCancel: vi.fn(), onSubmit } });

    const input = screen.getByLabelText(/player name/i) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: '   ' } });
    await fireEvent.submit(input.closest('form')!);

    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('fires onCancel from the cancel button', async () => {
    const onCancel = vi.fn();
    const onSubmit = vi.fn();
    render(AddOfflineAccountDialog, { props: { error: null, onCancel, onSubmit } });

    await fireEvent.click(screen.getByRole('button', { name: /^cancel$/i }));

    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('fires onCancel when Escape is pressed (Modal close)', async () => {
    const onCancel = vi.fn();
    const onSubmit = vi.fn();
    render(AddOfflineAccountDialog, { props: { error: null, onCancel, onSubmit } });

    await fireEvent.keyDown(window, { key: 'Escape' });

    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('renders a backend error when one is passed', () => {
    render(AddOfflineAccountDialog, {
      props: { error: 'That name is already taken', onCancel: vi.fn(), onSubmit: vi.fn() },
    });
    expect(screen.getByRole('alert').textContent).toMatch(/already taken/i);
  });
});
