import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import RemoveAccountDialog from '$lib/accounts/RemoveAccountDialog.svelte';

describe('RemoveAccountDialog', () => {
  it('names the account being removed', () => {
    render(RemoveAccountDialog, {
      props: { accountName: 'Tester', onCancel: vi.fn(), onConfirm: vi.fn() },
    });
    // The question interpolates the account name. getByText throws if absent.
    expect(screen.getByText(/remove tester\?/i)).toBeTruthy();
  });

  it('fires onConfirm from the danger button only', async () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(RemoveAccountDialog, { props: { accountName: 'Tester', onCancel, onConfirm } });

    const confirmBtn = screen.getByRole('button', { name: /^remove$/i });
    expect(confirmBtn).toHaveBtnVariant('danger');
    await fireEvent.click(confirmBtn);

    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onCancel).not.toHaveBeenCalled();
  });

  it('fires onCancel from the cancel button only', async () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(RemoveAccountDialog, { props: { accountName: 'Tester', onCancel, onConfirm } });

    await fireEvent.click(screen.getByRole('button', { name: /^cancel$/i }));

    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
