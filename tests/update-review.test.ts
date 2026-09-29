import { describe, expect, it } from 'vitest';
import type { Translate } from '$lib/i18n';
import { buildReviewItems, updatesReport } from '$lib/mods/installed/update-review';

const tr = ((k: string, v?: Record<string, unknown>) =>
  v ? `${k} ${JSON.stringify(v)}` : k) as unknown as Translate;
const ok = (name: string, deps: string[]) => ({
  sha1: name.toLowerCase(),
  name,
  ok: true as const,
  summary: { primary_name: name, installed_dependencies: deps, details: [] },
});
const failed = (name: string, reason: string) => ({
  sha1: name.toLowerCase(),
  name,
  ok: false as const,
  reason,
});

describe('updatesReport', () => {
  it('counts the updates and lists each dependency they brought in once', () => {
    expect(updatesReport(tr, [ok('A', ['Lib', 'Api']), ok('B', ['Lib'])])).toEqual({
      kind: 'success',
      title: 'mods.installed.toastUpdated {"count":2}',
      lines: ['mods.updates.installedDeps {"names":"Lib, Api"}'],
    });
  });

  it('turns into a warning that names each failure with its reason', () => {
    const t = updatesReport(tr, [ok('A', []), failed('B', 'Network error')]);
    expect(t?.kind).toBe('warning');
    expect(t?.title).toBe('mods.installed.toastUpdatedFailed {"count":1,"failed":1}');
    expect(t?.lines).toEqual(['B: Network error']);
  });

  it('failures with the same reason share one line (a busy profile refuses every one alike)', () => {
    const t = updatesReport(tr, [failed('A', 'Busy'), failed('B', 'Busy'), failed('C', 'Gone')]);
    expect(t?.lines).toEqual(['A, B: Busy', 'C: Gone']);
  });

  it('a row update that failed is named, with its reason', () => {
    expect(updatesReport(tr, [failed('B', 'Busy')], { single: true })).toEqual({
      kind: 'warning',
      title: 'mods.updates.updateFailed {"name":"B"}',
      lines: ['Busy'],
    });
  });

  it('names the profile when the run ended on another one', () => {
    const t = updatesReport(tr, [ok('A', [])], { profile: 'Alpha Pack' });
    expect(t?.lines).toEqual(['mods.ops.restore.inProfile {"profile":"Alpha Pack"}']);
  });

  it('says nothing for an empty batch', () => expect(updatesReport(tr, [])).toBeNull());
});

describe('buildReviewItems', () => {
  const row = (sha1: string, source: string | null, name: string) => ({
    summary: null,
    installed: {
      sha1,
      source,
      project_id: `p${sha1}`,
      version_id: 'v1',
      version_number: '1.0',
      name,
    } as never,
  });
  const upd = (sha1: string) =>
    ({
      sha1,
      state: { kind: 'update_available', target: { version_id: 'v2', version_number: '2.0' } },
    }) as never;
  it('lists pending updates by name, with a changelog only where the source publishes one', () => {
    const checks = new Map([
      ['a', upd('a')],
      ['b', upd('b')],
      ['c', { sha1: 'c', state: { kind: 'up_to_date' } } as never],
    ]);
    const items = buildReviewItems(
      [row('a', 'modrinth', 'Zeta'), row('b', 'ftb', 'Alpha'), row('c', 'modrinth', 'Mid')],
      checks,
    );
    expect(items.map((i) => [i.name, i.from, i.to, i.changelog?.targetVersionId ?? null])).toEqual([
      ['Alpha', '1.0', '2.0', null],
      ['Zeta', '1.0', '2.0', 'v2'],
    ]);
  });
});
