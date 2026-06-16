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
    });
    expect(items[0].kind).toBe('log_issue');
  });
});
