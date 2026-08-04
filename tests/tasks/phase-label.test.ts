import { get } from 'svelte/store';
import { describe, expect, it } from 'vitest';
import { t } from '$lib/i18n';
import { phaseLabel } from '$lib/tasks/phase-label';
import type { Task } from '$lib/tasks/types';

// Real translator (not a stub) — same idiom tests/i18n-ru-plural-forms.test.ts
// uses — so these tests pin the ACTUAL reused copy, not a mock's echo.
const tr = get(t);

const base: Task = {
  id: 't',
  kind: 'game-install',
  scope: {},
  title: 'Some Task',
  phase: null,
  progress: null,
  rate: null,
  state: 'running',
  lane: 'concurrent',
  caps: { cancellable: false, reorderable: false },
  details: null,
  startedAt: 0,
  finishedAt: null,
};

describe('phaseLabel', () => {
  it('maps a game-install phase to the reused install.phase text', () => {
    const task: Task = { ...base, kind: 'game-install', phase: 'libraries' };
    expect(phaseLabel(tr, task)).toBe('Downloading libraries');
  });

  it('returns null for game-install before the first tick', () => {
    const task: Task = { ...base, kind: 'game-install', phase: null };
    expect(phaseLabel(tr, task)).toBeNull();
  });

  it('maps a mod-install phase to the reused install.modPhase text', () => {
    const task: Task = { ...base, kind: 'mod-install', phase: 'downloading' };
    expect(phaseLabel(tr, task)).toBe('Downloading mod');
  });

  it('maps a mod-update phase to the same install.modPhase text', () => {
    const task: Task = { ...base, kind: 'mod-update', phase: 'verifying' };
    expect(phaseLabel(tr, task)).toBe('Verifying mod');
  });

  it('maps pack-import "inspecting" with no args', () => {
    const task: Task = { ...base, kind: 'pack-import', phase: 'inspecting' };
    expect(phaseLabel(tr, task)).toBe('Inspecting pack…');
  });

  it('maps pack-import "creating_instance" using the task title as {name}', () => {
    const task: Task = {
      ...base,
      kind: 'pack-import',
      phase: 'creating_instance',
      title: 'My Pack',
    };
    expect(phaseLabel(tr, task)).toBe('Creating instance My Pack…');
  });

  it('maps pack-import "extracting_overrides" using progress for {current}/{total}', () => {
    const task: Task = {
      ...base,
      kind: 'pack-import',
      phase: 'extracting_overrides',
      progress: { current: 503, total: 765, unit: 'files' },
    };
    expect(phaseLabel(tr, task)).toBe('Extracting overrides 503/765…');
  });

  it('returns null for pack-import "installing_file" — the key needs a file name the registry never carries', () => {
    const task: Task = {
      ...base,
      kind: 'pack-import',
      phase: 'installing_file',
      progress: { current: 3, total: 10, unit: 'files' },
    };
    expect(phaseLabel(tr, task)).toBeNull();
  });

  it('maps pack-update through the same ModpackProgress phase space', () => {
    const task: Task = {
      ...base,
      kind: 'pack-update',
      phase: 'extracting_overrides',
      progress: { current: 1, total: 2, unit: 'files' },
    };
    expect(phaseLabel(tr, task)).toBe('Extracting overrides 1/2…');
  });

  it('maps launcher-import phases with no args needed', () => {
    const copying: Task = { ...base, kind: 'launcher-import', phase: 'copying' };
    expect(phaseLabel(tr, copying)).toBe('Copying files…');
    const creating: Task = { ...base, kind: 'launcher-import', phase: 'creating_instance' };
    expect(phaseLabel(tr, creating)).toBe('Creating instance…');
  });

  it('maps clone using the category label plus progress', () => {
    const task: Task = {
      ...base,
      kind: 'clone',
      phase: 'mods',
      progress: { current: 3, total: 10, unit: 'files' },
    };
    expect(phaseLabel(tr, task)).toBe('Copying Mods (3/10)');
  });

  it('returns null for clone before the first tick', () => {
    const task: Task = { ...base, kind: 'clone', phase: null, progress: null };
    expect(phaseLabel(tr, task)).toBeNull();
  });

  it('maps data-migration, including "preparing" for the pre-tick null phase', () => {
    expect(phaseLabel(tr, { ...base, kind: 'data-migration', phase: null })).toBe('Preparing…');
    expect(phaseLabel(tr, { ...base, kind: 'data-migration', phase: 'copying' })).toBe(
      'Copying files…',
    );
    expect(phaseLabel(tr, { ...base, kind: 'data-migration', phase: 'verifying' })).toBe(
      'Verifying copy…',
    );
    expect(phaseLabel(tr, { ...base, kind: 'data-migration', phase: 'deleting' })).toBe(
      'Removing old data…',
    );
  });

  it('has no phase vocabulary for verify, repair, server-upload, or app-update', () => {
    expect(phaseLabel(tr, { ...base, kind: 'verify', phase: 'hashing' })).toBeNull();
    expect(phaseLabel(tr, { ...base, kind: 'repair', phase: 'repairing' })).toBeNull();
    expect(
      phaseLabel(tr, { ...base, kind: 'server-upload', phase: '/some/file/path.jar' }),
    ).toBeNull();
    expect(phaseLabel(tr, { ...base, kind: 'app-update', phase: null })).toBeNull();
  });
});
