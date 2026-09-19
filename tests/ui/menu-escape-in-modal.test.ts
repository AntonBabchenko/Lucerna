import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import MenuInModal from '../fixtures/MenuInModal.svelte';

describe('Menu hosted by a Modal', () => {
  // Control: proves the modal's window-level Escape listener is reachable in
  // this DOM environment. Without it, the next test could pass vacuously.
  it('Escape with no menu open closes the modal', async () => {
    const onModalClose = vi.fn();
    render(MenuInModal, { props: { onModalClose } });
    await fireEvent.keyDown(screen.getByTestId('row'), { key: 'Escape' });
    expect(onModalClose).toHaveBeenCalledOnce();
  });

  it('Escape with the menu open closes only the menu', async () => {
    const onModalClose = vi.fn();
    render(MenuInModal, { props: { onModalClose } });
    await fireEvent.contextMenu(screen.getByTestId('row'));
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(onModalClose).not.toHaveBeenCalled();
  });
});
