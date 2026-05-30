import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

import ImportPickerDialog from '$lib/modpacks/ImportPickerDialog.svelte';

// Minimal happy-path fixture: one required mod (locked-checked) and one
// optional mod (default unchecked). Tests extend this for unresolvable
// rows and the saves-in-overrides warning.
const baseSummary = {
  format: 'modrinth' as const,
  name: 'Test Pack',
  version: '1.0',
  game_version: '1.20.1',
  loader: 'fabric' as const,
  loader_version: '0.15.7',
  files: [
    {
      project_id: 'p1',
      version_id: 'v1',
      name: 'Sodium',
      filename: 'sodium.jar',
      install_path: 'mods/sodium.jar',
      sha1: 'abc',
      url: 'https://cdn.modrinth.com/sodium.jar',
      size: 1_000_000,
      env_client: 'required' as const,
      source: 'modrinth' as const,
    },
    {
      project_id: 'p2',
      version_id: 'v2',
      name: 'Iris',
      filename: 'iris.jar',
      install_path: 'mods/iris.jar',
      sha1: 'def',
      url: 'https://cdn.modrinth.com/iris.jar',
      size: 500_000,
      env_client: 'optional' as const,
      source: 'modrinth' as const,
    },
  ],
  unresolvable: [],
  has_overrides: false,
  has_client_overrides: false,
  has_saves_in_overrides: false,
};

describe('ImportPickerDialog', () => {
  it('renders required and optional sections with counts', () => {
    const { getByText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(getByText(/Required \(1\)/)).toBeTruthy();
    expect(getByText(/Optional \(1\)/)).toBeTruthy();
    expect(getByText('Sodium')).toBeTruthy();
    expect(getByText('Iris')).toBeTruthy();
  });

  it('Install button counts required mods even when no optional is selected', () => {
    const { getByText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(getByText(/Install 1 selected/)).toBeTruthy();
  });

  it('toggling an optional updates the counter', async () => {
    const { getByText, getByLabelText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    await fireEvent.click(getByLabelText(/Install Iris/));
    expect(getByText(/Install 2 selected/)).toBeTruthy();
  });

  it('shows the saves-in-overrides warning when the flag is set', () => {
    const summary = { ...baseSummary, has_saves_in_overrides: true };
    const { getByText } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(getByText(/saved worlds/)).toBeTruthy();
  });

  it('renders the unresolvable list with an Open ↗ link', async () => {
    const summary = {
      ...baseSummary,
      unresolvable: [
        {
          reason: 'distribution_disabled' as const,
          mod_name: 'Embeddium',
          manual_action_url: 'https://www.curseforge.com/projects/911',
          filename: 'embeddium.jar',
          size: 100,
          sha1: null,
        },
      ],
    };
    const { getByText } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(getByText('Embeddium')).toBeTruthy();
    // The "Open ↗" control is now a <button> that routes through the Tauri
    // opener plugin — not an <a href> — to prevent javascript: injection from
    // upstream-controlled URLs.
    const btn = getByText('Open ↗') as HTMLButtonElement;
    expect(btn.tagName).toBe('BUTTON');
    expect(btn.getAttribute('href')).toBeNull();
    // Clicking must invoke openUrl via the plugin, not navigate the browser.
    openUrlMock.mockClear();
    await fireEvent.click(btn);
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith('https://www.curseforge.com/projects/911');
    });
  });

  it('fires onConfirm with the required + selected optional shas', async () => {
    const onConfirm = vi.fn();
    const { getByText, getByLabelText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm },
    });
    // Tick the optional so we exercise the [required, ...optional] order
    // the parent depends on for the modpack_import call.
    await fireEvent.click(getByLabelText(/Install Iris/));
    await fireEvent.click(getByText(/Install 2 selected/));
    expect(onConfirm).toHaveBeenCalledWith(['abc', 'def']);
  });

  it('fires onCancel when the Cancel button is clicked', async () => {
    const onCancel = vi.fn();
    const { getByText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel, onConfirm: () => {} },
    });
    await fireEvent.click(getByText('Cancel'));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('disables the Install button when there are zero selected files', () => {
    const summary = { ...baseSummary, files: [] };
    const { getByText } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    const button = getByText(/Install 0 selected/) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });
});
