import { beforeEach, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import type { PreLaunchCheck } from '$lib/ipc/bindings';
import { warningLines } from '$lib/launch/pre-launch-warning';

// Pin the locale so the English-text assertions below are deterministic
// regardless of the process/OS default the i18n bootstrap resolves to.
beforeEach(() => {
  locale.set('en');
});

describe('warningLines', () => {
  it('returns empty when no warnings', () => {
    const check: PreLaunchCheck = { resource_warning: null, account_conflict: null };
    expect(warningLines(check)).toEqual([]);
  });

  it('includes a resource line with reserved/total GB when over threshold', () => {
    const check: PreLaunchCheck = {
      resource_warning: { reserved_mb: 14336, total_mb: 16384 },
      account_conflict: null,
    };
    const lines = warningLines(check);
    expect(lines).toHaveLength(1);
    expect(lines[0]).toContain('14.0'); // 14336 / 1024
    expect(lines[0]).toContain('16.0'); // 16384 / 1024
  });

  it('includes the Microsoft copy for a microsoft conflict', () => {
    const check: PreLaunchCheck = {
      resource_warning: null,
      account_conflict: {
        account_name: 'Steve',
        running_instance_id: 'inst-1',
        account_kind: 'microsoft',
      },
    };
    const lines = warningLines(check);
    expect(lines).toHaveLength(1);
    expect(lines[0]).toContain('Steve');
    expect(lines[0]).toContain('online server');
    expect(lines[0]).not.toContain('duplicate username');
  });

  it('includes the offline copy for an offline conflict', () => {
    const check: PreLaunchCheck = {
      resource_warning: null,
      account_conflict: {
        account_name: 'Steve',
        running_instance_id: 'inst-1',
        account_kind: 'offline',
      },
    };
    const lines = warningLines(check);
    expect(lines).toHaveLength(1);
    expect(lines[0]).toContain('Steve');
    expect(lines[0]).toContain('duplicate username');
    expect(lines[0]).not.toContain('online server');
  });

  it('renders the RAM figures with the locale decimal mark', () => {
    const check: PreLaunchCheck = {
      resource_warning: { reserved_mb: 14336, total_mb: 16384 },
      account_conflict: null,
    };
    // `.toFixed(1)` produced an English "14.0" inside Russian copy. Both
    // arguments are checked: numericArgKeys used to keep only the first.
    locale.set('ru');
    const line = warningLines(check)[0];
    expect(line).toContain('14,0');
    expect(line).toContain('16,0');
    expect(line).not.toContain('14.0');
  });

  it('includes both lines when resource + account both present', () => {
    const check: PreLaunchCheck = {
      resource_warning: { reserved_mb: 14336, total_mb: 16384 },
      account_conflict: {
        account_name: 'Steve',
        running_instance_id: 'inst-1',
        account_kind: 'microsoft',
      },
    };
    const lines = warningLines(check);
    expect(lines).toHaveLength(2);
    // Both branches exercised: one line carries the RAM figures, the other the account name.
    expect(lines.some((l) => l.includes('16.0'))).toBe(true);
    expect(lines.some((l) => l.includes('Steve'))).toBe(true);
  });
});
