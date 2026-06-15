import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import QuickJoinDialog from '$lib/worlds/QuickJoinDialog.svelte';

const saved = [
  { name: 'SMP', address: 'play.example.net' },
  { name: 'Anom', address: 'mc.x:25566' },
];

describe('QuickJoinDialog', () => {
  beforeAll(() => locale.set('en'));

  it('lists saved servers and connects on row click', async () => {
    const onConnect = vi.fn();
    render(QuickJoinDialog, {
      open: true,
      savedServers: saved,
      connectDisabledReason: null,
      onConnect,
      onSave: vi.fn(),
      onSaveAndConnect: vi.fn(),
      onDelete: vi.fn(),
      onClose: vi.fn(),
    });
    expect(screen.getByText('SMP')).toBeTruthy();
    const connectButtons = screen.getAllByRole('button', { name: 'Connect' });
    await fireEvent.click(connectButtons[0]);
    expect(onConnect).toHaveBeenCalledWith('play.example.net');
  });

  it('disables connect when gated and shows the reason', () => {
    render(QuickJoinDialog, {
      open: true,
      savedServers: saved,
      connectDisabledReason: 'Quick Play requires 1.20+',
      onConnect: vi.fn(),
      onSave: vi.fn(),
      onSaveAndConnect: vi.fn(),
      onDelete: vi.fn(),
      onClose: vi.fn(),
    });
    const connectButtons = screen.getAllByRole('button', { name: 'Connect' });
    expect((connectButtons[0] as HTMLButtonElement).disabled).toBe(true);
  });

  it('Save calls onSave with name + address', async () => {
    const onSave = vi.fn();
    render(QuickJoinDialog, {
      open: true,
      savedServers: [],
      connectDisabledReason: null,
      onConnect: vi.fn(),
      onSave,
      onSaveAndConnect: vi.fn(),
      onDelete: vi.fn(),
      onClose: vi.fn(),
    });
    // Empty list → add section is expanded; fields are visible.
    await fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'New' } });
    await fireEvent.input(screen.getByLabelText('Address'), { target: { value: 'new.example' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(onSave).toHaveBeenCalledWith('New', 'new.example');
  });

  it('delete asks for confirmation before firing onDelete', async () => {
    const onDelete = vi.fn();
    render(QuickJoinDialog, {
      open: true,
      savedServers: saved,
      connectDisabledReason: null,
      onConnect: vi.fn(),
      onSave: vi.fn(),
      onSaveAndConnect: vi.fn(),
      onDelete,
      onClose: vi.fn(),
    });
    await fireEvent.click(screen.getAllByRole('button', { name: 'Delete server' })[0]);
    expect(onDelete).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(onDelete).toHaveBeenCalledWith(0, 'play.example.net');
  });
});
