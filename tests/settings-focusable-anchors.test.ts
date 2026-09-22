// Only a text field the user came to type into may take focus on a jump —
// a focus grab on a slider turns the next arrow key into a silent edit.
import { describe, expect, it } from 'vitest';
import {
  SETTINGS_SEARCH,
  type SettingsAnchor,
  shouldFocusAnchor,
} from '$lib/settings/search-index';

describe('shouldFocusAnchor', () => {
  it('is true for the CurseForge key field and false for every other anchor', () => {
    for (const anchor of Object.keys(SETTINGS_SEARCH) as SettingsAnchor[]) {
      expect(shouldFocusAnchor(anchor), anchor).toBe(anchor === 'integrations.curseforgeKey');
    }
  });
});
