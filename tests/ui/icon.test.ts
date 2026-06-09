import { describe, expect, it } from 'vitest';
import { ICON_NAMES, ICONS, type IconName } from '$lib/ui/icons';

describe('icon registry', () => {
  it('resolves a component for every registered name', () => {
    for (const name of ICON_NAMES) {
      expect(ICONS[name], `icon "${name}" must resolve`).toBeTruthy();
    }
  });

  it('exposes the names expected by the UI', () => {
    const expected: IconName[] = [
      'caret', 'chevronDown', 'chevronUp', 'close', 'warning', 'success',
      'download', 'update', 'refresh', 'plus', 'minus', 'list', 'grid',
    ];
    for (const n of expected) expect(ICON_NAMES).toContain(n);
  });
});
