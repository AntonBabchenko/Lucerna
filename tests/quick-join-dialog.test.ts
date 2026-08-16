import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import QuickJoinDialog from '$lib/worlds/QuickJoinDialog.svelte';

const saved = [
  { name: 'SMP', address: 'play.example.net' },
  { name: 'Anom', address: 'mc.x:25566' },
];

// onSave / onSaveAndConnect resolve `true` on success (the dialog clears +
// collapses the add section on a truthy result).
function baseProps(overrides: Record<string, unknown> = {}) {
  return {
    open: true,
    savedServers: saved,
    connectDisabledReason: null,
    onConnect: vi.fn(),
    onSave: vi.fn().mockResolvedValue(true),
    onSaveAndConnect: vi.fn().mockResolvedValue(true),
    onDelete: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
}

describe('QuickJoinDialog', () => {
  beforeAll(() => locale.set('en'));

  it('lists saved servers and connects on row click', async () => {
    const onConnect = vi.fn();
    render(QuickJoinDialog, baseProps({ onConnect }));
    expect(screen.getByText('SMP')).toBeTruthy();
    const connectButtons = screen.getAllByRole('button', { name: 'Connect' });
    await fireEvent.click(connectButtons[0]);
    expect(onConnect).toHaveBeenCalledWith('play.example.net');
  });

  it('disables connect when gated and shows the reason', () => {
    render(QuickJoinDialog, baseProps({ connectDisabledReason: 'Quick Play requires 1.20+' }));
    const connectButtons = screen.getAllByRole('button', { name: 'Connect' });
    expect((connectButtons[0] as HTMLButtonElement).disabled).toBe(true);
  });

  it('Save calls onSave with name + address', async () => {
    const onSave = vi.fn().mockResolvedValue(true);
    render(QuickJoinDialog, baseProps({ savedServers: [], onSave }));
    // Empty list → add section is expanded; fields are visible.
    await fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'New' } });
    await fireEvent.input(screen.getByLabelText('Address'), { target: { value: 'new.example' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(onSave).toHaveBeenCalledWith('New', 'new.example');
  });

  it('clears the form and collapses the add section after a successful save', async () => {
    render(QuickJoinDialog, baseProps({ savedServers: [] }));
    const nameInput = screen.getByLabelText('Name') as HTMLInputElement;
    const addressInput = screen.getByLabelText('Address') as HTMLInputElement;
    await fireEvent.input(nameInput, { target: { value: 'New' } });
    await fireEvent.input(addressInput, { target: { value: 'new.example' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    // The save resolves true → form clears and the <details> collapses.
    await waitFor(() => {
      expect(nameInput.value).toBe('');
      expect(addressInput.value).toBe('');
      const details = screen.getByText('Add a server').closest('details') as HTMLDetailsElement;
      expect(details.open).toBe(false);
    });
  });

  it('keeps the add section collapsed when opened with saved servers', () => {
    render(QuickJoinDialog, baseProps());
    // Non-empty list → add section starts collapsed. The <details> wrapping
    // the Name/Address inputs must not carry the `open` attribute, so the
    // fields are hidden by default (a `<details>` keeps its content in the DOM
    // even when closed, so assert on `open`, not DOM presence).
    const summary = screen.getByText('Add a server');
    const details = summary.closest('details');
    expect(details).not.toBeNull();
    expect((details as HTMLDetailsElement).open).toBe(false);
  });

  it('copies the server address to the clipboard', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    render(QuickJoinDialog, baseProps());
    await fireEvent.click(screen.getAllByRole('button', { name: 'Copy address' })[0]);
    expect(writeText).toHaveBeenCalledWith('play.example.net');
    vi.unstubAllGlobals();
  });

  it('delete asks for confirmation before firing onDelete', async () => {
    const onDelete = vi.fn();
    render(QuickJoinDialog, baseProps({ onDelete }));
    await fireEvent.click(screen.getAllByRole('button', { name: 'Delete server' })[0]);
    expect(onDelete).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(onDelete).toHaveBeenCalledWith(0, 'play.example.net');
  });

  it('shows the read failure instead of rendering it as an empty list', () => {
    // The page passes [] plus the formatted error when list_saved_servers fails
    // (typically servers_dat_parse on a truncated servers.dat).
    render(
      QuickJoinDialog,
      baseProps({
        savedServers: [],
        savedServersError: 'Could not read the server list (servers.dat): unexpected end of input',
      }),
    );

    const err = screen.getByTestId('quick-join-load-error');
    expect(err.textContent).toContain('servers.dat');
    // The "you have none" copy must NOT be shown — it is a different claim.
    expect(screen.queryByText('No saved servers yet. Add one below.')).toBeNull();
  });

  it('does not auto-expand Add a server after a failed read', () => {
    // With a genuinely empty list the dialog opens the add section (there would
    // be no other obvious action). After a failed read that would read as
    // "you have none" — which is exactly what we could not determine.
    render(
      QuickJoinDialog,
      baseProps({ savedServers: [], savedServersError: 'Could not read the server list' }),
    );
    const details = screen.getByText('Add a server').closest('details') as HTMLDetailsElement;
    expect(details.open).toBe(false);
  });

  it('still auto-expands Add a server when the list is genuinely empty', () => {
    // Guards the discrimination in the other direction, so the fix cannot be
    // "never auto-expand".
    render(QuickJoinDialog, baseProps({ savedServers: [], savedServersError: null }));
    const details = screen.getByText('Add a server').closest('details') as HTMLDetailsElement;
    expect(details.open).toBe(true);
  });
});
