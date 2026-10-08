import { describe, expect, it, vi } from 'vitest';
import type { DepRoot, DepTreeNode } from '$lib/ipc/bindings';
import {
  DEPS_TOUR_IDLE,
  type DepsTourInput,
  type DepsTourRow,
  type DepsTourTarget,
  depsTourSteps,
  nextDepsTourState,
  pickDepsTourTarget,
} from '$lib/mods/installed/deps-tour';
import { DEPS_STEPS } from '$lib/onboarding/contextual-tours';

const node = (pid: string, o: Partial<DepTreeNode> = {}): DepTreeNode => ({
  source: 'modrinth',
  project_id: pid,
  name: pid,
  installed: true,
  declared: 'required',
  cycle: false,
  children: [],
  ...o,
});
const root = (
  sha1: string,
  pid: string,
  required: DepTreeNode[] = [],
  optional: DepTreeNode[] = [],
  o: Partial<DepRoot> = {},
): DepRoot => ({ sha1, source: 'modrinth', project_id: pid, name: pid, required, optional, ...o });
const row = (
  sha1: string,
  pid: string,
  r: DepRoot | undefined,
  depTotal: number,
  enabled = true,
): DepsTourRow => ({ sha1, enabled, source: 'modrinth', projectId: pid, root: r, depTotal });

describe('pickDepsTourTarget', () => {
  it('finds nothing on a page without a mod that requires another', () => {
    expect(pickDepsTourTarget([row('l', 'PL', root('l', 'PL'), 0)])).toBeNull();
    expect(pickDepsTourTarget([])).toBeNull();
  });

  it('skips a switched-off mod, a mod without a graph root and one whose dependencies are unknown', () => {
    const r = root('a', 'PA', [node('PL')]);
    expect(pickDepsTourTarget([row('a', 'PA', r, 1, false)])).toBeNull();
    expect(pickDepsTourTarget([row('a', 'PA', undefined, 1)])).toBeNull();
    const unknown = root('a', 'PA', [node('PL')], [], { deps_unknown: 'unreachable' });
    expect(pickDepsTourTarget([row('a', 'PA', unknown, 1)])).toBeNull();
  });

  it('prefers a mod whose installed dependency is on the page, even below one whose is not', () => {
    const page = [
      row('x', 'PX', root('x', 'PX', [node('ELSEWHERE')]), 1),
      row('a', 'PA', root('a', 'PA', [node('PL')]), 1),
      row('l', 'PL', root('l', 'PL'), 0),
    ];
    expect(pickDepsTourTarget(page)).toEqual({
      modSha1: 'a',
      librarySha1: 'l',
      hasOptional: false,
    });
  });

  it('takes the first candidate without a library when none has one on the page', () => {
    const optional = [node('OPT', { installed: false, declared: 'optional' })];
    const page = [
      row('x', 'PX', root('x', 'PX', [node('ELSEWHERE')], optional), 1),
      row('y', 'PY', root('y', 'PY', [node('ALSO-ELSEWHERE')]), 1),
    ];
    expect(pickDepsTourTarget(page)).toEqual({
      modSha1: 'x',
      librarySha1: null,
      hasOptional: true,
    });
  });

  it('never takes an absent dependency, a switched-off row or the mod itself as the library', () => {
    const required = [node('PA', { cycle: true }), node('GONE', { installed: false }), node('PL')];
    const page = [
      row('a', 'PA', root('a', 'PA', required), 2),
      row('l', 'PL', root('l', 'PL'), 0, false),
    ];
    expect(pickDepsTourTarget(page)).toEqual({
      modSha1: 'a',
      librarySha1: null,
      hasOptional: false,
    });
  });
});

describe('depsTourSteps', () => {
  const t = (librarySha1: string | null, hasOptional: boolean): DepsTourTarget => ({
    modSha1: 'a',
    librarySha1,
    hasOptional,
  });

  it('keeps all four in order when everything is there', () => {
    expect(depsTourSteps(t('l', true))).toEqual([...DEPS_STEPS]);
  });

  it('leaves Optional out when the mod has none', () => {
    expect(depsTourSteps(t('l', false))).toEqual([DEPS_STEPS[0], DEPS_STEPS[1], DEPS_STEPS[3]]);
  });

  it('leaves Required by out when no library is on the page', () => {
    expect(depsTourSteps(t(null, true))).toEqual([DEPS_STEPS[0], DEPS_STEPS[1], DEPS_STEPS[2]]);
    expect(depsTourSteps(t(null, false))).toEqual([DEPS_STEPS[0], DEPS_STEPS[1]]);
  });
});

describe('nextDepsTourState — one attempt per entry', () => {
  const T: DepsTourTarget = { modSha1: 'a', librarySha1: null, hasOptional: false };
  const on: DepsTourInput = {
    onInstalled: true,
    instanceId: 'i1',
    screenFree: true,
    settled: true,
    pick: () => T,
  };

  it('picks at once on an entry when the list has settled and the screen is free', () => {
    expect(nextDepsTourState(DEPS_TOUR_IDLE, on)).toEqual({
      kind: 'held',
      instanceId: 'i1',
      target: T,
    });
  });

  it('waits, armed, while the list settles or another surface owns the screen', () => {
    const pick = vi.fn(() => T);
    const armed = nextDepsTourState(DEPS_TOUR_IDLE, { ...on, settled: false, pick });
    expect(armed).toEqual({ kind: 'armed', instanceId: 'i1' });
    expect(nextDepsTourState(armed, { ...on, screenFree: false, pick })).toBe(armed);
    expect(nextDepsTourState(armed, { ...on, settled: false, pick })).toBe(armed);
    expect(pick).not.toHaveBeenCalled();
    expect(nextDepsTourState(armed, { ...on, pick })).toEqual({
      kind: 'held',
      instanceId: 'i1',
      target: T,
    });
  });

  it('spends the attempt when there is nothing to show, and does not try again before a re-entry', () => {
    const spent = nextDepsTourState(DEPS_TOUR_IDLE, { ...on, pick: () => null });
    expect(spent).toEqual({ kind: 'spent', instanceId: 'i1' });
    expect(nextDepsTourState(spent, on)).toBe(spent);
    const left = nextDepsTourState(spent, { ...on, onInstalled: false });
    expect(left).toBe(DEPS_TOUR_IDLE);
    expect(nextDepsTourState(left, on)).toEqual({ kind: 'held', instanceId: 'i1', target: T });
  });

  it('keeps its target through its own tour, a reload, paging and a screen owner', () => {
    const held = nextDepsTourState(DEPS_TOUR_IDLE, on);
    const other = { ...on, pick: () => ({ ...T, modSha1: 'z' }) };
    expect(nextDepsTourState(held, { ...other, screenFree: false })).toBe(held);
    expect(nextDepsTourState(held, { ...other, settled: false })).toBe(held);
    expect(nextDepsTourState(held, other)).toBe(held);
  });

  it('drops the target and tries again on a profile switch while on the list', () => {
    const held = nextDepsTourState(DEPS_TOUR_IDLE, on);
    expect(nextDepsTourState(held, { ...on, instanceId: 'i2', settled: false })).toEqual({
      kind: 'armed',
      instanceId: 'i2',
    });
  });

  it('goes idle when the list leaves the screen', () => {
    const held = nextDepsTourState(DEPS_TOUR_IDLE, on);
    expect(nextDepsTourState(held, { ...on, onInstalled: false })).toBe(DEPS_TOUR_IDLE);
    expect(nextDepsTourState(DEPS_TOUR_IDLE, { ...on, onInstalled: false })).toBe(DEPS_TOUR_IDLE);
  });
});
