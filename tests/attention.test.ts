import { describe, expect, it } from 'vitest';
import { buildAttentionItems } from '$lib/overview/attention';

describe('attention: log_issue', () => {
  it('adds a log_issue item when a log problem is present', () => {
    const items = buildAttentionItems({
      mcVersionMissing: false,
      missingModsCount: 0,
      incompatibleCount: 0,
      integrityProblemCount: 0,
      hasModpackUpdate: false,
      hasLogIssue: true,
      logFixAvailable: false,
    });
    expect(items.some((i) => i.kind === 'log_issue')).toBe(true);
  });

  it('omits log_issue when no log problem', () => {
    const items = buildAttentionItems({
      mcVersionMissing: false,
      missingModsCount: 0,
      incompatibleCount: 0,
      integrityProblemCount: 0,
      hasModpackUpdate: false,
      hasLogIssue: false,
      logFixAvailable: false,
    });
    expect(items.some((i) => i.kind === 'log_issue')).toBe(false);
  });

  it('places log_issue first when both log issue and pick_version are present', () => {
    const items = buildAttentionItems({
      mcVersionMissing: true,
      missingModsCount: 0,
      incompatibleCount: 0,
      integrityProblemCount: 0,
      hasModpackUpdate: false,
      hasLogIssue: true,
      logFixAvailable: false,
    });
    expect(items[0].kind).toBe('log_issue');
  });
});

describe('attention: log_fix', () => {
  const base = {
    mcVersionMissing: false,
    missingModsCount: 0,
    incompatibleCount: 0,
    integrityProblemCount: 0,
    hasModpackUpdate: false,
  };

  it('emits log_fix (not log_issue) when the log issue has a repair', () => {
    const items = buildAttentionItems({ ...base, hasLogIssue: true, logFixAvailable: true });
    expect(items.some((i) => i.kind === 'log_fix')).toBe(true);
    expect(items.some((i) => i.kind === 'log_issue')).toBe(false);
  });

  it('emits log_issue (not log_fix) when the log issue has no repair', () => {
    const items = buildAttentionItems({ ...base, hasLogIssue: true, logFixAvailable: false });
    expect(items.some((i) => i.kind === 'log_issue')).toBe(true);
    expect(items.some((i) => i.kind === 'log_fix')).toBe(false);
  });

  it('emits neither when there is no log issue', () => {
    const items = buildAttentionItems({ ...base, hasLogIssue: false, logFixAvailable: true });
    expect(items.some((i) => i.kind === 'log_issue' || i.kind === 'log_fix')).toBe(false);
  });
});
