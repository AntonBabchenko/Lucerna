/**
 * Tests for PreflightGateDialog: verifies that all three buttons are present,
 * that they call the correct handlers, and that they are disabled when busy.
 *
 * i18n resolves to real EN strings in the test environment — use actual text
 * values from en.json rather than key paths.
 */
import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { PreflightReport } from '$lib/ipc/bindings';
import PreflightGateDialog from '$lib/mods/PreflightGateDialog.svelte';

const report: PreflightReport = {
  violations: [
    {
      kind: 'version_out_of_range',
      dependent_name: 'Sophisticated Backpacks',
      dependent_sha1: 'aa',
      dep_id: 'sophisticatedcore',
      dep_display_name: 'Sophisticated Core',
      needed: '[1.3.51,)',
      installed_version: '1.3.50',
      provider_project: { source: 'modrinth', project_id: 'core-id', version_id: null },
    },
  ],
};

const defaultProps = {
  report,
  onUpdateLaunch: vi.fn(),
  onLaunchAnyway: vi.fn(),
  onCancel: vi.fn(),
};

describe('PreflightGateDialog', () => {
  it('renders the gate title ("Some mods may not load")', () => {
    const { getByText } = render(PreflightGateDialog, { props: defaultProps });
    expect(getByText('Some mods may not load')).toBeTruthy();
  });

  it('renders the preflight panel with violation rows', () => {
    const { getByTestId } = render(PreflightGateDialog, { props: defaultProps });
    expect(getByTestId('preflight-panel')).toBeTruthy();
    expect(getByTestId('preflight-row')).toBeTruthy();
  });

  it('has data-testid="preflight-gate-dialog" on the dialog panel', () => {
    const { getByTestId } = render(PreflightGateDialog, { props: defaultProps });
    expect(getByTestId('preflight-gate-dialog')).toBeTruthy();
  });

  it('calls onCancel when the Cancel button is clicked', async () => {
    const onCancel = vi.fn();
    const { getByRole } = render(PreflightGateDialog, {
      props: { ...defaultProps, onCancel },
    });
    await fireEvent.click(getByRole('button', { name: /cancel/i }));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('calls onLaunchAnyway when the "Launch anyway" button is clicked', async () => {
    const onLaunchAnyway = vi.fn();
    const { getByRole } = render(PreflightGateDialog, {
      props: { ...defaultProps, onLaunchAnyway },
    });
    await fireEvent.click(getByRole('button', { name: /launch anyway/i }));
    expect(onLaunchAnyway).toHaveBeenCalledOnce();
  });

  it('calls onUpdateLaunch when the "Update & launch" button is clicked', async () => {
    const onUpdateLaunch = vi.fn();
    const { getByRole } = render(PreflightGateDialog, {
      props: { ...defaultProps, onUpdateLaunch },
    });
    await fireEvent.click(getByRole('button', { name: /update & launch/i }));
    expect(onUpdateLaunch).toHaveBeenCalledOnce();
  });

  it('disables Cancel and Launch anyway buttons when busy=true', () => {
    const { getAllByRole } = render(PreflightGateDialog, {
      props: { ...defaultProps, busy: true },
    });
    const buttons = getAllByRole('button');
    // Dialog has 4 buttons: panel "Update" row-btn, dialog "Cancel", "Launch anyway", "Update & launch"
    const cancelBtn = buttons.find((b) => b.textContent?.trim() === 'Cancel');
    const anywayBtn = buttons.find((b) => /launch anyway/i.test(b.textContent ?? ''));
    expect(cancelBtn).toBeDefined();
    expect(anywayBtn).toBeDefined();
    expect(cancelBtn?.hasAttribute('disabled')).toBe(true);
    expect(anywayBtn?.hasAttribute('disabled')).toBe(true);
  });

  it('disables the Update & launch button when busy=true (BusyButton sets disabled)', () => {
    const { getAllByRole } = render(PreflightGateDialog, {
      props: { ...defaultProps, busy: true },
    });
    const buttons = getAllByRole('button');
    // The "Update & launch" BusyButton has aria-busy=true; plain "Update" panel row btn does not
    const updateLaunchBtn = buttons.find((b) => b.getAttribute('aria-busy') === 'true');
    expect(updateLaunchBtn).toBeDefined();
    expect(updateLaunchBtn?.hasAttribute('disabled')).toBe(true);
  });
});
