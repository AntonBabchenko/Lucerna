/**
 * Tests for PreflightGateDialog: verifies that all three buttons are present,
 * that they call the correct handlers, and that they are disabled when busy.
 *
 * i18n resolves to real EN strings in the test environment — use actual text
 * values from en.json rather than key paths.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DepViolation, PreflightReport } from '$lib/ipc/bindings';

const h = vi.hoisted(() => ({ modsResolveDepNames: vi.fn() }));
// The gate names dependencies from the module name store (dep-names.svelte.ts), which this command
// fills — for the Installed tab, and for the gate itself as it opens (never waited on).
vi.mock('$lib/ipc/bindings', () => ({ commands: { modsResolveDepNames: h.modsResolveDepNames } }));

import { __resetDepNamesForTests, resolveDepNames } from '$lib/mods/dep-names.svelte';
import PreflightGateDialog from '$lib/mods/PreflightGateDialog.svelte';
import { rawRangeDesc } from './test-utils/range-desc';

const outOfRange: DepViolation = {
  kind: 'version_out_of_range',
  dependent_name: 'Sophisticated Backpacks',
  dependent_sha1: 'aa',
  dep_id: 'sophisticatedcore',
  needed: '[1.3.51,)',
  needed_desc: rawRangeDesc('[1.3.51,)'),
  installed_version: '1.3.50',
  provider_project: { source: 'modrinth', project_id: 'core-id', version_id: null },
  provider_sha1: null,
  family: 'maven',
};
const report: PreflightReport = { violations: [outOfRange] };
// Sophisticated Backpacks needs `balm`, which nothing provides.
const missingBalm: PreflightReport = {
  violations: [
    {
      ...outOfRange,
      kind: 'missing_required',
      dep_id: 'balm',
      needed: '',
      needed_desc: rawRangeDesc(''),
      installed_version: null,
      provider_project: null,
      family: null,
    },
  ],
};

const defaultProps = {
  report,
  onFixAndLaunch: vi.fn(),
  onLaunchAnyway: vi.fn(),
  onCancel: vi.fn(),
};

afterEach(() => __resetDepNamesForTests());

describe('PreflightGateDialog', () => {
  it('has ONE heading — «What stops the game» — and it names the dialog', () => {
    render(PreflightGateDialog, { props: defaultProps });
    expect(screen.getByRole('dialog', { name: 'What stops the game' })).toBeTruthy();
    // The panel's own title would be a second heading saying the same thing.
    expect(screen.getAllByText('What stops the game')).toHaveLength(1);
    expect(screen.queryByText('Some mods may not load')).toBeNull();
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

  it('calls onFixAndLaunch when "Fix and launch" is clicked', async () => {
    const onFixAndLaunch = vi.fn();
    const { getByRole } = render(PreflightGateDialog, {
      props: { ...defaultProps, onFixAndLaunch },
    });
    await fireEvent.click(getByRole('button', { name: /fix and launch/i }));
    expect(onFixAndLaunch).toHaveBeenCalledOnce();
  });

  it('disables Cancel and Launch anyway buttons when busy=true', () => {
    const { getAllByRole } = render(PreflightGateDialog, {
      props: { ...defaultProps, busy: true },
    });
    const buttons = getAllByRole('button');
    // Buttons: "Cancel", "Launch anyway", "Fix and launch" (the panel's row actions are muted)
    const cancelBtn = buttons.find((b) => b.textContent?.trim() === 'Cancel');
    const anywayBtn = buttons.find((b) => /launch anyway/i.test(b.textContent ?? ''));
    expect(cancelBtn).toBeDefined();
    expect(anywayBtn).toBeDefined();
    expect(cancelBtn?.hasAttribute('disabled')).toBe(true);
    expect(anywayBtn?.hasAttribute('disabled')).toBe(true);
  });

  it('disables Fix and launch when busy=true (BusyButton sets disabled)', () => {
    const { getAllByRole } = render(PreflightGateDialog, {
      props: { ...defaultProps, busy: true },
    });
    const buttons = getAllByRole('button');
    // The "Fix and launch" BusyButton is the one with aria-busy=true
    const fixLaunchBtn = buttons.find((b) => b.getAttribute('aria-busy') === 'true');
    expect(fixLaunchBtn).toBeDefined();
    expect(fixLaunchBtn?.hasAttribute('disabled')).toBe(true);
  });

  it('offers «Fix and launch» only when some row is fixable', () => {
    // Positive half first — without it the absence below is green before the change too.
    const { unmount } = render(PreflightGateDialog, { props: defaultProps });
    expect(screen.getByRole('button', { name: /fix and launch/i })).toBeTruthy();
    unmount();
    const platformOnly: PreflightReport = {
      violations: [
        {
          ...outOfRange,
          kind: 'platform_mismatch',
          dep_id: 'minecraft',
          provider_project: null,
        },
      ],
    };
    render(PreflightGateDialog, { props: { ...defaultProps, report: platformOnly } });
    expect(screen.queryByRole('button', { name: /fix and launch/i })).toBeNull();
    expect(screen.getByRole('button', { name: /launch anyway/i })).toBeTruthy();
  });

  it('says how much a partial repair fixed', () => {
    const { getByTestId } = render(PreflightGateDialog, {
      props: { ...defaultProps, fixed: { fixed: 1, total: 3, reasons: [] } },
    });
    expect(getByTestId('preflight-gate-fixed').textContent?.trim()).toBe('Fixed 1 of 3');
  });

  // «Fixed 0 of 2» alone cannot tell a held profile from unrelated failures.
  it('says why the steps that failed failed, under «Fixed N of M», and in the same announcement', () => {
    const busy =
      'Another operation — such as a modpack update, a migration or a clone — is using this profile. Try again once it finishes.';
    const { getByTestId } = render(PreflightGateDialog, {
      props: { ...defaultProps, fixed: { fixed: 0, total: 2, reasons: [busy, 'No connection'] } },
    });
    const region = getByTestId('preflight-gate-fixed');
    expect(region.querySelector('p')?.textContent?.trim()).toBe('Fixed 0 of 2');
    const lines = [...region.querySelectorAll('li')].map((li) => li.textContent?.trim());
    expect(lines).toEqual([busy, 'No connection']);
  });

  it('parks focus on the dialog body when the repair starts (DESIGN.md §8)', async () => {
    // A focused button that turns disabled drops focus to <body>, and Tab then walks the page
    // behind the dialog.
    const { rerender } = render(PreflightGateDialog, { props: defaultProps });
    screen.getByRole('button', { name: /fix and launch/i }).focus();
    await rerender({ ...defaultProps, busy: true });
    expect(document.activeElement).toBe(screen.getByTestId('preflight-gate-body'));
  });

  it('names a dependency resolved earlier at once, and asks nothing it already knows', async () => {
    h.modsResolveDepNames.mockResolvedValue({
      status: 'ok',
      data: [{ dependent_sha1: 'aa', dep_id: 'balm', name: 'Balm', project: null }],
    });
    await resolveDepNames('inst', missingBalm);
    h.modsResolveDepNames.mockClear();
    render(PreflightGateDialog, {
      props: { ...defaultProps, report: missingBalm, instanceId: 'inst' },
    });
    expect(screen.getByTestId('preflight-row').textContent).toContain(
      'Sophisticated Backpacks needs Balm, which is not installed',
    );
    expect(h.modsResolveDepNames).not.toHaveBeenCalled();
  });

  // 07c: a gate opened on a cold start knows no names. It asks as it opens — the same cache-first
  // resolver the Installed tab uses — and never waits: the dialog and its buttons are there at once,
  // and the row shows the loader id only until the name arrives.
  it('asks for a missing dependency’s name as it opens, showing the id until it arrives', async () => {
    let answer: (r: unknown) => void = () => {};
    h.modsResolveDepNames.mockReturnValue(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );
    render(PreflightGateDialog, {
      props: { ...defaultProps, report: missingBalm, instanceId: 'cold' },
    });
    const row = screen.getByTestId('preflight-row');
    expect(row.textContent).toContain('Sophisticated Backpacks needs balm, which is not installed');
    const anyway = screen.getByRole('button', { name: /launch anyway/i }) as HTMLButtonElement;
    expect(anyway.disabled).toBe(false);
    await waitFor(() =>
      expect(h.modsResolveDepNames).toHaveBeenCalledWith('cold', [
        { dependent_sha1: 'aa', dep_id: 'balm' },
      ]),
    );
    answer({
      status: 'ok',
      data: [{ dependent_sha1: 'aa', dep_id: 'balm', name: 'Balm', project: null }],
    });
    await waitFor(() =>
      expect(row.textContent).toContain(
        'Sophisticated Backpacks needs Balm, which is not installed',
      ),
    );
  });
});
