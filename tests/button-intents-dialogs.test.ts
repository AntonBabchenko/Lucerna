import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { World } from '$lib/ipc/bindings';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    deleteWorld: vi.fn(),
  },
}));

import CloseButton from '$lib/ui/CloseButton.svelte';
import DeleteWorldDialog from '$lib/worlds/DeleteWorldDialog.svelte';

function makeWorld(over: Partial<World> = {}): World {
  return {
    folder_name: 'New World',
    size_bytes: null,
    modified_unix_ms: null,
    backup_count: 0,
    ...over,
  };
}

describe('CloseButton primitive', () => {
  it('renders as btn-icon and preserves aria-label', () => {
    render(CloseButton, { props: { ariaLabel: 'Close mod details' } });
    const btn = screen.getByRole('button', { name: /close mod details/i });
    expect(btn).toHaveBtnVariant('icon');
    expect(btn.getAttribute('aria-label')).toBe('Close mod details');
  });
});

describe('DeleteWorldDialog — confirm pair', () => {
  const dialogProps = {
    instanceId: 'inst-1',
    world: makeWorld(),
    onClose: () => {},
    onDeleted: () => {},
  };

  it('Cancel is btn-secondary btn-sm', () => {
    render(DeleteWorldDialog, { props: dialogProps });
    const cancel = screen.getByRole('button', { name: /cancel/i });
    expect(cancel).toHaveBtnVariant('secondary');
    expect(cancel).toHaveBtnSize('sm');
  });

  it('Delete is btn-danger btn-sm', () => {
    render(DeleteWorldDialog, { props: dialogProps });
    const deleteBtn = screen.getByRole('button', { name: /^delete$/i });
    expect(deleteBtn).toHaveBtnVariant('danger');
    expect(deleteBtn).toHaveBtnSize('sm');
  });
});
