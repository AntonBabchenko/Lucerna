// What an integrity result means everywhere (spec 2026-10-18): notes — Forge files built
// differently, or not checked — are not damage; any damage counts as it always has.
import { describe, expect, it } from 'vitest';
import { summarizeIntegrity } from '$lib/instances/integrity-summary';
import type { CategoryReport } from '$lib/ipc/bindings';

const libs = (over: Partial<CategoryReport> = {}): CategoryReport => ({
  category: 'libraries',
  total: 40,
  ok: 40,
  missing: 0,
  corrupt: 0,
  built_differently: 0,
  ...over,
});

describe('summarizeIntegrity', () => {
  it('a healthy result is ok', () => {
    const s = summarizeIntegrity({ healthy: true, problem_count: 0, categories: [libs()] });
    expect(s.state).toBe('ok');
  });

  it('files built differently and nothing else are notes', () => {
    const s = summarizeIntegrity({
      healthy: false,
      problem_count: 3,
      categories: [libs({ ok: 37, built_differently: 3 })],
      forge_installer: null,
    });
    expect(s).toEqual({ state: 'notes', notes: 3, problemCount: 3 });
  });

  it('an unchecked Forge installer alone is a note, not a missing library', () => {
    const s = summarizeIntegrity({
      healthy: false,
      problem_count: 1,
      categories: [libs()],
      forge_installer: 'not_saved',
    });
    expect(s.state).toBe('notes');
  });

  it('any damage counts as damage, notes included in the count', () => {
    const s = summarizeIntegrity({
      healthy: false,
      problem_count: 4,
      categories: [libs({ ok: 36, missing: 1, built_differently: 3 })],
      forge_installer: null,
    });
    expect(s).toEqual({ state: 'damaged', notes: 3, problemCount: 4 });
  });

  it('an unhealthy result with nothing counted (an unreadable manifest) is damage', () => {
    const s = summarizeIntegrity({ healthy: false, problem_count: 0, categories: [] });
    expect(s.state).toBe('damaged');
  });
});
