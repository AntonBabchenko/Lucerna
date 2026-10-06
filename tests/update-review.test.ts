import { describe, expect, it } from 'vitest';
import type { Translate } from '$lib/i18n';
import { buildReviewItems, oldVersionKept, updatesReport } from '$lib/mods/installed/update-review';

const tr = ((k: string, v?: Record<string, unknown>) =>
  v ? `${k} ${JSON.stringify(v)}` : k) as unknown as Translate;
const ok = (name: string, deps: string[]) => ({
  sha1: name.toLowerCase(),
  name,
  ok: true as const,
  summary: { primary_name: name, installed_dependencies: deps, details: [] },
});
const failed = (name: string, reason: string, previousKept = false) => ({
  sha1: name.toLowerCase(),
  name,
  ok: false as const,
  reason,
  previousKept,
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
    // The names on a line above the reason (`reasonLines`, plan §5d L4).
    expect(t?.lines).toEqual([{ names: 'B', reason: 'Network error' }]);
  });

  it('failures with the same reason share one line (a busy profile refuses every one alike)', () => {
    const t = updatesReport(tr, [failed('A', 'Busy'), failed('B', 'Busy'), failed('C', 'Gone')]);
    expect(t?.lines).toEqual([
      { names: 'A, B', reason: 'Busy' },
      { names: 'C', reason: 'Gone' },
    ]);
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

  it('says the previous version is still there when every failed update was undone', () => {
    expect(updatesReport(tr, [failed('B', 'Name taken', true)], { single: true })?.lines).toEqual([
      'Name taken',
      'mods.updates.previousKept',
    ]);
    const batch = updatesReport(tr, [ok('A', []), failed('B', 'Gone', true)]);
    expect(batch?.lines).toEqual([{ names: 'B', reason: 'Gone' }, 'mods.updates.previousKept']);
  });

  it('does not claim the previous version is there when one failure is not known to be undone', () => {
    const t = updatesReport(tr, [failed('A', 'Gone', true), failed('B', 'Bridge died', false)]);
    expect(t?.lines).not.toContain('mods.updates.previousKept');
  });
});

describe('oldVersionKept', () => {
  it('holds for an error the update returned after undoing its own changes', () => {
    expect(oldVersionKept({ kind: 'mods_filename_conflict', filename: 'a.jar' })).toBe(true);
    expect(oldVersionKept({ kind: 'network', details: 'timed out' })).toBe(true);
  });

  it('is not claimed for a refusal: another operation owns the profile and its files may be moving', () => {
    expect(oldVersionKept({ kind: 'instance_busy' })).toBe(false);
  });

  it('is not claimed while an unfinished earlier update may hold files aside', () => {
    expect(oldVersionKept({ kind: 'content_update_unfinished', folder: 'x' })).toBe(false);
  });

  it('is not claimed when the undo left files behind', () => {
    expect(
      oldVersionKept({
        kind: 'content_update_rollback_incomplete',
        folder: 'x',
        cause: { kind: 'instance_busy' },
        stuck: ['y'],
      }),
    ).toBe(false);
  });

  it('is not claimed for an error the backend does not send', () => {
    expect(oldVersionKept('boom')).toBe(false);
    expect(oldVersionKept(null)).toBe(false);
    expect(oldVersionKept({})).toBe(false);
  });
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
