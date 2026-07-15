import { describe, expect, it } from 'vitest';
import type { PreLaunchCheck } from '$lib/ipc/bindings';
import { warningLines } from '$lib/launch/pre-launch-warning';

describe('warningLines', () => {
  it('returns empty when no warnings', () => {
    const check: PreLaunchCheck = { resource_warning: null, account_conflict: null };
    expect(warningLines(check)).toEqual([]);
  });

  it('includes a resource line when over threshold', () => {
    const check: PreLaunchCheck = {
      resource_warning: { reserved_mb: 14336, total_mb: 16384 },
      account_conflict: null,
    };
    expect(warningLines(check)).toHaveLength(1);
  });

  it('includes an account line for a microsoft conflict', () => {
    const check: PreLaunchCheck = {
      resource_warning: null,
      account_conflict: {
        account_name: 'Steve',
        running_instance_id: 'inst-1',
        account_kind: 'microsoft',
      },
    };
    expect(warningLines(check)).toHaveLength(1);
  });

  it('includes an account line for an offline conflict', () => {
    const check: PreLaunchCheck = {
      resource_warning: null,
      account_conflict: {
        account_name: 'Steve',
        running_instance_id: 'inst-1',
        account_kind: 'offline',
      },
    };
    expect(warningLines(check)).toHaveLength(1);
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
    expect(warningLines(check)).toHaveLength(2);
  });
});
