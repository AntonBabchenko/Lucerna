/**
 * The dependency-name store (spec 2026-09-28 §6.4): one module-level answer per instance and
 * (dependent, dep id), shared by every surface that shows a pre-flight row. Dependent-scoped
 * because two mods may declare the same bare mod-id and mean different projects (audit A-F3).
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DepViolation, ViolationKind } from '$lib/ipc/bindings';
import {
  __resetDepNamesForTests,
  depNameEntry,
  depNameOf,
  depProjectOf,
  resolveDepNames,
} from '$lib/mods/dep-names.svelte';
import { rawRangeDesc } from './test-utils/range-desc';

const h = vi.hoisted(() => ({ modsResolveDepNames: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({ commands: { modsResolveDepNames: h.modsResolveDepNames } }));

const v = (dependent: string, depId: string, kind: ViolationKind): DepViolation => ({
  dependent_sha1: dependent,
  dependent_name: dependent.toUpperCase(),
  dep_id: depId,
  kind,
  installed_version: null,
  needed: '',
  needed_desc: rawRangeDesc(''),
  provider_project: null,
  provider_sha1: null,
  family: null,
});
const report = (...violations: DepViolation[]) => ({ violations });
const resolved = (dependent: string, depId: string, name: string) => ({
  dependent_sha1: dependent,
  dep_id: depId,
  name,
  project: { source: 'modrinth' as const, project_id: `p-${name}` },
});

beforeEach(() => {
  __resetDepNamesForTests();
  h.modsResolveDepNames.mockReset();
  h.modsResolveDepNames.mockResolvedValue({ status: 'ok', data: [] });
});

describe('dependency-name store', () => {
  it('asks for missing AND disabled requirements, once per (dependent, dep)', async () => {
    await resolveDepNames(
      'i',
      report(
        v('a', 'lib', 'missing_required'),
        v('a', 'lib', 'missing_required'),
        v('b', 'core', 'required_disabled'),
        v('c', 'api', 'version_out_of_range'),
      ),
    );
    expect(h.modsResolveDepNames).toHaveBeenCalledTimes(1);
    expect(h.modsResolveDepNames).toHaveBeenCalledWith('i', [
      { dependent_sha1: 'a', dep_id: 'lib' },
      { dependent_sha1: 'b', dep_id: 'core' },
    ]);
  });

  it('keys names by dependent, so two mods sharing a mod-id keep their own answers', async () => {
    h.modsResolveDepNames.mockResolvedValue({
      status: 'ok',
      data: [resolved('a', 'lib', 'Lib for Alpha'), resolved('b', 'lib', 'Lib for Beta')],
    });
    await resolveDepNames(
      'i',
      report(v('a', 'lib', 'missing_required'), v('b', 'lib', 'missing_required')),
    );
    expect(depNameOf('i', 'a', 'lib')).toBe('Lib for Alpha');
    expect(depNameOf('i', 'b', 'lib')).toBe('Lib for Beta');
    expect(depProjectOf('i', 'a', 'lib')).toEqual({
      source: 'modrinth',
      project_id: 'p-Lib for Alpha',
    });
    expect(depNameEntry('i', 'b', 'lib')).toEqual({
      name: 'Lib for Beta',
      project: { source: 'modrinth', project_id: 'p-Lib for Beta' },
    });
    // Per instance, and nothing without one.
    expect(depNameOf('other', 'a', 'lib')).toBeNull();
    expect(depProjectOf(null, 'a', 'lib')).toBeNull();
  });

  it('does not ask again for a name it already has', async () => {
    h.modsResolveDepNames.mockResolvedValue({ status: 'ok', data: [resolved('a', 'lib', 'Lib')] });
    const r = report(v('a', 'lib', 'missing_required'));
    await resolveDepNames('i', r);
    await resolveDepNames('i', r);
    expect(h.modsResolveDepNames).toHaveBeenCalledTimes(1);
  });

  // Every mod toggle re-runs the pre-flight, so a new report can arrive while the names of the
  // last one are still being asked for. The pair already asked about joins that call; only the
  // new pair costs a request, and the promise settles once both answers are in.
  it('joins a call that is still out instead of asking again for the same pair', async () => {
    let answer: (r: unknown) => void = () => {};
    h.modsResolveDepNames.mockReturnValueOnce(
      new Promise((r) => {
        answer = r;
      }),
    );
    const first = resolveDepNames('i', report(v('a', 'lib', 'missing_required')));
    h.modsResolveDepNames.mockResolvedValueOnce({
      status: 'ok',
      data: [resolved('b', 'core', 'Core')],
    });
    let secondSettled = false;
    const second = resolveDepNames(
      'i',
      report(v('a', 'lib', 'missing_required'), v('b', 'core', 'missing_required')),
    ).then(() => {
      secondSettled = true;
    });

    expect(h.modsResolveDepNames).toHaveBeenCalledTimes(2);
    expect(h.modsResolveDepNames).toHaveBeenLastCalledWith('i', [
      { dependent_sha1: 'b', dep_id: 'core' },
    ]);
    await new Promise((r) => setTimeout(r, 0));
    expect(secondSettled).toBe(false); // still waiting for a/lib

    answer({ status: 'ok', data: [resolved('a', 'lib', 'Lib')] });
    await Promise.all([first, second]);
    expect(depNameOf('i', 'a', 'lib')).toBe('Lib');
    expect(depNameOf('i', 'b', 'core')).toBe('Core');
  });

  it('an unresolved name stays absent, and a failed call changes nothing and does not throw', async () => {
    await resolveDepNames('i', report(v('a', 'lib', 'missing_required')));
    expect(depNameOf('i', 'a', 'lib')).toBeNull();

    h.modsResolveDepNames.mockRejectedValueOnce(new Error('bridge gone'));
    await expect(
      resolveDepNames('i', report(v('a', 'x', 'missing_required'))),
    ).resolves.toBeUndefined();
    expect(depNameOf('i', 'a', 'x')).toBeNull();

    h.modsResolveDepNames.mockResolvedValueOnce({ status: 'error', error: { kind: 'network' } });
    await expect(
      resolveDepNames('i', report(v('a', 'y', 'missing_required'))),
    ).resolves.toBeUndefined();
    expect(depNameOf('i', 'a', 'y')).toBeNull();

    // Unresolved is not remembered as "no name": the next report asks again.
    h.modsResolveDepNames.mockResolvedValueOnce({ status: 'ok', data: [resolved('a', 'x', 'X')] });
    await resolveDepNames('i', report(v('a', 'x', 'missing_required')));
    expect(depNameOf('i', 'a', 'x')).toBe('X');
  });

  it('keeps a blank answer out: the raw id is the honest fallback, never an empty name', async () => {
    h.modsResolveDepNames.mockResolvedValue({
      status: 'ok',
      data: [resolved('a', 'lib', '  ')],
    });
    await resolveDepNames('i', report(v('a', 'lib', 'missing_required')));
    expect(depNameOf('i', 'a', 'lib')).toBeNull();
  });
});
