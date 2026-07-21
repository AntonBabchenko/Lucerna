import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';

// Guards the shared confirm primitive against intent drift, the same way
// button-intents-dialogs.test.ts guards the individual dialogs: the confirm
// button must track `variant` and Cancel must stay btn-secondary — both at
// btn-sm. If this breaks, every migrated confirm dialog silently drifts.
describe('ConfirmDialog — confirm/cancel intents', () => {
  const base = {
    title: 'Delete thing?',
    bodyText: 'This cannot be undone.',
    confirmLabel: 'Remove',
    onCancel: () => {},
    onConfirm: () => {},
  };

  it('danger variant renders a btn-danger btn-sm confirm', () => {
    render(ConfirmDialog, { props: { ...base, variant: 'danger' } });
    const confirm = screen.getByRole('button', { name: /remove/i });
    expect(confirm).toHaveBtnVariant('danger');
    expect(confirm).toHaveBtnSize('sm');
  });

  it('primary variant renders a btn-primary btn-sm confirm', () => {
    render(ConfirmDialog, { props: { ...base, variant: 'primary', confirmLabel: 'Save' } });
    const confirm = screen.getByRole('button', { name: /save/i });
    expect(confirm).toHaveBtnVariant('primary');
    expect(confirm).toHaveBtnSize('sm');
  });

  it('Cancel is btn-secondary btn-sm', () => {
    render(ConfirmDialog, { props: base });
    const cancel = screen.getByRole('button', { name: /cancel/i });
    expect(cancel).toHaveBtnVariant('secondary');
    expect(cancel).toHaveBtnSize('sm');
  });
});
