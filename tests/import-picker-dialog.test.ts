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

// Summary with files in multiple categories
const multiCategorySummary = {
  ...baseSummary,
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
      project_id: 'p3',
      version_id: 'v3',
      name: 'OptiFine Config',
      filename: 'optifine.cfg',
      install_path: 'config/optifine.cfg',
      sha1: 'ghi',
      url: 'https://cdn.modrinth.com/optifine.cfg',
      size: 512,
      env_client: 'required' as const,
      source: 'modrinth' as const,
    },
  ],
};

// ── formatSize unit tests ─────────────────────────────────────────────────
describe('formatSize (ImportPickerDialog internal)', () => {
  // We test via the rendered size output. The component renders sizes in the
  // file list, so we verify through the DOM.

  it('renders 0 size as empty (no size span text)', () => {
    const summary = {
      ...baseSummary,
      files: [
        {
          ...baseSummary.files[0],
          size: 0,
        },
      ],
    };
    const { container } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    // The size span should be empty when size is 0
    const sizeSpans = container.querySelectorAll('.text-placeholder.text-xs');
    for (const span of sizeSpans) {
      expect(span.textContent).toBe('');
    }
  });

  it('renders null size as empty', () => {
    const summary = {
      ...baseSummary,
      files: [
        {
          ...baseSummary.files[0],
          size: null,
        },
      ],
    };
    const { container } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    const sizeSpans = container.querySelectorAll('.text-placeholder.text-xs');
    for (const span of sizeSpans) {
      expect(span.textContent).toBe('');
    }
  });

  it('renders 500 bytes as "500 B"', () => {
    const summary = {
      ...baseSummary,
      files: [
        {
          ...baseSummary.files[0],
          install_path: 'mods/tiny.jar',
          size: 500,
        },
      ],
    };
    const { getByText } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(getByText('500 B')).toBeTruthy();
  });

  it('renders 2048 bytes as "2.0 KiB"', () => {
    const summary = {
      ...baseSummary,
      files: [
        {
          ...baseSummary.files[0],
          install_path: 'mods/small.jar',
          size: 2048,
        },
      ],
    };
    const { getByText } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(getByText('2.0 KiB')).toBeTruthy();
  });

  it('renders 2*1024*1024 bytes as "2.0 MiB"', () => {
    const summary = {
      ...baseSummary,
      files: [
        {
          ...baseSummary.files[0],
          install_path: 'mods/big.jar',
          size: 2 * 1024 * 1024,
        },
      ],
    };
    const { getByText } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(getByText('2.0 MiB')).toBeTruthy();
  });
});

// ── Category grouping tests ───────────────────────────────────────────────
describe('ImportPickerDialog category groups', () => {
  it('renders a Mods group with <details open>', () => {
    const { container } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    const details = container.querySelectorAll('details');
    expect(details.length).toBeGreaterThan(0);
    // The Mods group (first details) must be open by default
    const modsDetails = details[0];
    expect(modsDetails.hasAttribute('open')).toBe(true);
  });

  it('Mods group contains both Sodium and Iris', () => {
    const { getByText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(getByText('Sodium')).toBeTruthy();
    expect(getByText('Iris')).toBeTruthy();
  });

  it('renders two groups for a summary with mods + config files', () => {
    const { container } = render(ImportPickerDialog, {
      props: { summary: multiCategorySummary, onCancel: () => {}, onConfirm: () => {} },
    });
    const details = container.querySelectorAll('details');
    // Mods group + Config group (no unresolvable)
    expect(details.length).toBe(2);
  });

  it('Config group is collapsed by default (no open attribute)', () => {
    const { container } = render(ImportPickerDialog, {
      props: { summary: multiCategorySummary, onCancel: () => {}, onConfirm: () => {} },
    });
    const details = container.querySelectorAll('details');
    // details[0] = Mods (open), details[1] = Config (not open)
    expect(details[0].hasAttribute('open')).toBe(true);
    expect(details[1].hasAttribute('open')).toBe(false);
  });

  it('renders the group summary header text including count', () => {
    const { getByText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    // Both files are in "mods/" so there should be one Mods group with 2 files
    // The summary text contains "Mods (2 · ...)"
    const summaryEl = getByText(/Mods \(2/);
    expect(summaryEl).toBeTruthy();
  });
});

// ── Selection logic tests ────────────────────────────────────────────────
describe('ImportPickerDialog selection', () => {
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
    const btn = getByText('Open') as HTMLButtonElement;
    expect(btn.tagName).toBe('BUTTON');
    expect(btn.getAttribute('href')).toBeNull();
    // Clicking must invoke openUrl via the plugin, not navigate the browser.
    openUrlMock.mockClear();
    await fireEvent.click(btn);
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith('https://www.curseforge.com/projects/911');
    });
  });

  it('unresolvable group is collapsed by default', () => {
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
    const { container } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    // Last details is the unresolvable group
    const details = container.querySelectorAll('details');
    const last = details[details.length - 1];
    expect(last.hasAttribute('open')).toBe(false);
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

  it('deduplicates sha1s when FTB packs have duplicate sha1s', async () => {
    // Two required files sharing the same sha1 (FTB pack pattern)
    const summary = {
      ...baseSummary,
      files: [
        {
          ...baseSummary.files[0],
          install_path: 'mods/mod-a.jar',
          name: 'Mod A',
          sha1: 'dupe',
        },
        {
          ...baseSummary.files[0],
          install_path: 'mods/mod-b.jar',
          name: 'Mod B',
          sha1: 'dupe',
        },
      ],
    };
    const onConfirm = vi.fn();
    const { getByText } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm },
    });
    // Should show "Install 1 selected" not "Install 2 selected"
    expect(getByText(/Install 1 selected/)).toBeTruthy();
    await fireEvent.click(getByText(/Install 1 selected/));
    expect(onConfirm).toHaveBeenCalledWith(['dupe']);
  });
});
