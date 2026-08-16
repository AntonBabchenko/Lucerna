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

  it('renders 2048 bytes as "2.0 KB"', () => {
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
    expect(getByText('2.0 KB')).toBeTruthy();
  });

  it('renders 2*1024*1024 bytes as "2.0 MB"', () => {
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
    expect(getByText('2.0 MB')).toBeTruthy();
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
  it('checks every file by default (required + optional) in the counter', () => {
    const { getByText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    // Sodium (required) + Iris (optional) both start checked.
    expect(getByText(/Install 2 selected/)).toBeTruthy();
  });

  it('unchecking a file decrements the counter', async () => {
    const { getByText, getByLabelText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    await fireEvent.click(getByLabelText(/Install Iris/)); // was checked → off
    expect(getByText(/Install 1 selected/)).toBeTruthy();
  });

  it('fires onConfirm with all shas by default', async () => {
    const onConfirm = vi.fn();
    const { getByText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm },
    });
    await fireEvent.click(getByText(/Install 2 selected/));
    expect(onConfirm).toHaveBeenCalledWith(['abc', 'def']);
  });

  it('omits an unchecked file from onConfirm', async () => {
    const onConfirm = vi.fn();
    const { getByText, getByLabelText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm },
    });
    await fireEvent.click(getByLabelText(/Install Iris/)); // uncheck optional
    await fireEvent.click(getByText(/Install 1 selected/));
    expect(onConfirm).toHaveBeenCalledWith(['abc']);
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

  it('refuses to open a non-https manual_action_url from a crafted pack', async () => {
    const summary = {
      ...baseSummary,
      unresolvable: [
        {
          reason: 'host_not_allowed' as const,
          mod_name: 'Evil',
          // modrinth.rs preserves the manifest's raw download URL verbatim for
          // host_not_allowed entries — the scheme is attacker-controlled.
          manual_action_url: 'javascript:alert(1)',
          filename: 'evil.jar',
          size: 100,
          sha1: null,
        },
      ],
    };
    const { getByText } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    openUrlMock.mockClear();
    await fireEvent.click(getByText('Open'));
    // The opener plugin must never see a non-https scheme.
    await new Promise((r) => setTimeout(r, 20));
    expect(openUrlMock).not.toHaveBeenCalled();
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

  it('an optional resource pack is selected by default', async () => {
    const summary = {
      ...baseSummary,
      files: [
        ...baseSummary.files,
        {
          project_id: 'rp',
          version_id: 'rpv',
          name: 'Faithful',
          filename: 'Faithful.zip',
          install_path: 'resourcepacks/Faithful.zip',
          sha1: 'rpsha',
          url: 'https://cdn.modrinth.com/Faithful.zip',
          size: 2048,
          env_client: 'optional' as const,
          source: 'modrinth' as const,
        },
      ],
    };
    const onConfirm = vi.fn();
    const { getByText } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm },
    });
    // 3 files, all checked → resource pack sha present in the payload.
    await fireEvent.click(getByText(/Install 3 selected/));
    expect(onConfirm).toHaveBeenCalledWith(['abc', 'def', 'rpsha']);
  });

  it('the select-all checkbox deselects everything then restores it', async () => {
    const { getByText, getByTestId } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    // Default = all selected → the unified checkbox is checked; unchecking clears.
    await fireEvent.click(getByTestId('picker-select-all'));
    const installBtn = getByText(/Install 0 selected/) as HTMLButtonElement;
    expect(installBtn.disabled).toBe(true);
    // Checking again restores everything.
    await fireEvent.click(getByTestId('picker-select-all'));
    expect(getByText(/Install 2 selected/)).toBeTruthy();
  });

  it('shows a soft warning when a required mod is unchecked', async () => {
    const { getByText, getByLabelText, queryByText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(queryByText(/Required mods unchecked/)).toBeNull();
    await fireEvent.click(getByLabelText(/Install Sodium/)); // Sodium is required
    expect(getByText(/Required mods unchecked: 1/)).toBeTruthy();
  });
});

// ── Required-signal scoping ──────────────────────────────────────────────
// `env_client: 'required'` means "shipped by the author", not "needed to
// launch" — the .mrpack parser defaults to it when `env` is absent, and FTB /
// ATLauncher tag every file that way. Only a dropped mod can break the launch,
// so the "may not launch" warning and the Required badge are mods-only.
describe('ImportPickerDialog required-signal scoping', () => {
  // A required shaderpack alongside a required mod — the shaderpack must never
  // claim it can stop the game from starting.
  const withShader = {
    ...baseSummary,
    files: [
      baseSummary.files[0], // Sodium — required mod
      {
        project_id: 'sp',
        version_id: 'spv',
        name: 'ComplementaryReimagined',
        filename: 'ComplementaryReimagined.zip',
        install_path: 'shaderpacks/ComplementaryReimagined.zip',
        sha1: 'shsha',
        url: 'https://cdn.modrinth.com/ComplementaryReimagined.zip',
        size: 534_000,
        env_client: 'required' as const,
        source: 'modrinth' as const,
      },
    ],
  };

  it('unchecking a required shaderpack does NOT claim the pack may not launch', async () => {
    const { getByText, getByLabelText, queryByText } = render(ImportPickerDialog, {
      props: { summary: withShader, onCancel: () => {}, onConfirm: () => {} },
    });
    await fireEvent.click(getByLabelText(/Install ComplementaryReimagined/));
    expect(queryByText(/may not launch/)).toBeNull();
    expect(getByText(/Pack files unchecked: 1/)).toBeTruthy();
  });

  it('unchecking a required mod still warns about the launch', async () => {
    const { getByText, getByLabelText, queryByText } = render(ImportPickerDialog, {
      props: { summary: withShader, onCancel: () => {}, onConfirm: () => {} },
    });
    await fireEvent.click(getByLabelText(/Install Sodium/));
    expect(getByText(/Required mods unchecked: 1 — the pack may not launch/)).toBeTruthy();
    expect(queryByText(/Pack files unchecked/)).toBeNull();
  });

  it('counts the two categories independently when both are unchecked', async () => {
    const { getByText, getByLabelText } = render(ImportPickerDialog, {
      props: { summary: withShader, onCancel: () => {}, onConfirm: () => {} },
    });
    await fireEvent.click(getByLabelText(/Install Sodium/));
    await fireEvent.click(getByLabelText(/Install ComplementaryReimagined/));
    expect(getByText(/Required mods unchecked: 1/)).toBeTruthy();
    expect(getByText(/Pack files unchecked: 1/)).toBeTruthy();
  });

  it('renders the Required badge on mods but not on non-mod files', () => {
    const { getAllByText } = render(ImportPickerDialog, {
      props: { summary: withShader, onCancel: () => {}, onConfirm: () => {} },
    });
    // Both files are env_client: 'required'; only the mod row earns the badge.
    expect(getAllByText('Required')).toHaveLength(1);
  });

  it('counts a duplicated FTB sha1 once, not once per install path', async () => {
    // FTB packs repeat a sha1 across install paths and the checkboxes are keyed
    // by sha1, so one click unchecks both rows. The banner must say 1, not 2.
    const dupeSummary = {
      ...baseSummary,
      files: [
        { ...baseSummary.files[0], install_path: 'mods/mod-a.jar', name: 'Mod A', sha1: 'dupe' },
        { ...baseSummary.files[0], install_path: 'mods/mod-b.jar', name: 'Mod B', sha1: 'dupe' },
      ],
    };
    const { getByText, getByLabelText } = render(ImportPickerDialog, {
      props: { summary: dupeSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    await fireEvent.click(getByLabelText(/Install Mod A/));
    expect(getByText(/Required mods unchecked: 1/)).toBeTruthy();
  });

  it('keeps the unchecked notices inside a persistent live region', async () => {
    const { container, getByLabelText } = render(ImportPickerDialog, {
      props: { summary: withShader, onCancel: () => {}, onConfirm: () => {} },
    });
    // The region exists before any banner does, so a later appearance is announced.
    const region = container.querySelector('[role="status"][aria-atomic="true"]');
    expect(region).toBeTruthy();
    expect(region?.textContent?.trim()).toBe('');
    await fireEvent.click(getByLabelText(/Install Sodium/));
    expect(region?.textContent).toContain('may not launch');
  });
});

// ── optional badge ────────────────────────────────────────────────────────
describe('optional badge', () => {
  it('labels an explicitly-optional mod with the Optional badge', () => {
    const { getAllByText } = render(ImportPickerDialog, {
      props: { summary: baseSummary, onCancel: () => {}, onConfirm: () => {} },
    });
    // Iris is env_client: 'optional'; Sodium (required) keeps its own badge.
    expect(getAllByText('Optional')).toHaveLength(1);
    expect(getAllByText('Required')).toHaveLength(1);
  });

  it('labels an explicitly-optional non-mod file too (author metadata, any group)', () => {
    const summary = {
      ...baseSummary,
      files: [
        {
          ...baseSummary.files[0],
          name: 'Complementary Shaders',
          filename: 'shader.zip',
          install_path: 'shaderpacks/shader.zip',
          sha1: 'sh1',
          env_client: 'optional' as const,
        },
      ],
    };
    const { getAllByText } = render(ImportPickerDialog, {
      props: { summary, onCancel: () => {}, onConfirm: () => {} },
    });
    expect(getAllByText('Optional')).toHaveLength(1);
  });
});
