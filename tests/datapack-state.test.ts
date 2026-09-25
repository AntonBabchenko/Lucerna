// The ignored-row vocabulary shared by the world tab, the library screen and
// the server pane (spec §2 N.6/N.7, §0.5 A12). One label and one hint per
// `IgnoredReason`, each resolved through an exhaustive switch so a new reason
// cannot ship without its copy.
import { describe, expect, it } from 'vitest';
import type { IgnoredReason } from '$lib/ipc/bindings';
import { ignoredHintKey, ignoredLabelKey } from '$lib/worlds/datapack-state';

const ALL: IgnoredReason[] = [
  'folder_without_pack_mcmeta',
  'folder_pack_nested_inside',
  'zip_extension_not_lowercase',
  'zip_without_pack_mcmeta',
  'unreadable',
  'not_loadable',
];

describe('ignoredLabelKey', () => {
  it('says "Couldn\'t check" only for an entry Lucerna could not read', () => {
    for (const r of ALL)
      expect(ignoredLabelKey(r)).toBe(
        r === 'unreadable'
          ? 'worlds.datapacks.stateCouldNotCheck'
          : 'worlds.datapacks.stateIgnored',
      );
  });
});

describe('ignoredHintKey', () => {
  it('names its own reason for every scan-derived reason', () => {
    expect(ignoredHintKey('folder_without_pack_mcmeta')).toBe(
      'worlds.datapacks.ignoredReason.folderWithoutPackMcmeta',
    );
    expect(ignoredHintKey('folder_pack_nested_inside')).toBe(
      'worlds.datapacks.ignoredReason.folderPackNestedInside',
    );
    expect(ignoredHintKey('zip_extension_not_lowercase')).toBe(
      'worlds.datapacks.ignoredReason.zipExtensionNotLowercase',
    );
    expect(ignoredHintKey('zip_without_pack_mcmeta')).toBe(
      'worlds.datapacks.ignoredReason.zipWithoutPackMcmeta',
    );
    expect(ignoredHintKey('unreadable')).toBe('worlds.datapacks.ignoredReason.unreadable');
    expect(ignoredHintKey(null)).toBeNull();
  });
});
