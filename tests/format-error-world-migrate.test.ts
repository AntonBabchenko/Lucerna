import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import type { Error as IpcError } from '$lib/ipc/bindings';
import { ERROR_CLASS, formatError } from '$lib/ipc/format-error';

// The two migration variants exist because their copy must name the
// operation and the instance honestly (world-migration spec §8): the reused
// import key opens with "Import failed" and says "inside saves" with no
// instance — read from the source's Worlds tab, that points at the user's
// original world. These tests pin what each sentence must carry, in both
// locales, across a live language switch (svelte-i18n memoises its formatter
// by message string, so a mid-session switch is what catches a locked one).
describe('formatError — world migration variants', () => {
  beforeAll(() => locale.set('en'));
  afterAll(() => locale.set('en'));

  it('classifies both variants as clean (built from structured fields only)', () => {
    expect(ERROR_CLASS.world_migrate_partial_left).toBe('clean');
    expect(ERROR_CLASS.world_migrate_instance_running).toBe('clean');
  });

  it('partial_left names the stage folder and the TARGET instance, en → ru → en', () => {
    const e: IpcError = {
      kind: 'world_migrate_partial_left',
      folder_name: '.tmp-migrate-copy-Base-1',
      target_instance: 'Survival 1.21',
      only_copy: false,
    };

    const en = formatError(e);
    expect(en).toContain('.tmp-migrate-copy-Base-1');
    expect(en).toContain('Survival 1.21');
    // The sentence the import key lacks: the original is safe.
    expect(en).toContain('untouched');
    // Never the import wording — this is a migration.
    expect(en).not.toContain('Import');
    expect(en.startsWith('errors.')).toBe(false);

    locale.set('ru');
    const ru = formatError(e);
    expect(ru).toContain('.tmp-migrate-copy-Base-1');
    expect(ru).toContain('Survival 1.21');
    expect(ru).toContain('не тронут');
    expect(ru).not.toContain('Импорт');
    expect(ru).not.toBe(en);

    locale.set('en');
    expect(formatError(e)).toBe(en);
  });

  it('only_copy=true tells the user to put the parked world back, never to delete it, en → ru → en', () => {
    const e: IpcError = {
      kind: 'world_migrate_partial_left',
      folder_name: '.tmp-migrate-moved-Base-0',
      target_instance: 'Survival 1.21',
      only_copy: true,
    };
    const en = formatError(e);
    expect(en).toContain('.tmp-migrate-moved-Base-0');
    expect(en).toContain('Survival 1.21');
    expect(en).toContain('do not delete');
    expect(en).not.toContain('Delete the');

    locale.set('ru');
    const ru = formatError(e);
    expect(ru).toContain('не удаляйте');
    expect(ru).not.toContain('Удалите');

    locale.set('en');
    expect(formatError(e)).toBe(en);
  });

  it('instance_running renders the role as a translated word, en → ru → en', () => {
    const source: IpcError = {
      kind: 'world_migrate_instance_running',
      instance_name: 'Creative Lab',
      role: 'source',
    };
    const target: IpcError = {
      kind: 'world_migrate_instance_running',
      instance_name: 'Creative Lab',
      role: 'target',
    };

    const enSource = formatError(source);
    expect(enSource).toBe('Stop "Creative Lab" first — it is the source of this migration.');
    expect(formatError(target)).toBe(
      'Stop "Creative Lab" first — it is the target of this migration.',
    );

    locale.set('ru');
    const ruSource = formatError(source);
    const ruTarget = formatError(target);
    expect(ruSource).toBe('Сначала остановите «Creative Lab» — это источник этой миграции.');
    expect(ruTarget).toBe('Сначала остановите «Creative Lab» — это цель этой миграции.');
    // The serde tag must never reach a Russian user.
    expect(ruSource).not.toContain('source');
    expect(ruTarget).not.toContain('target');

    locale.set('en');
    expect(formatError(source)).toBe(enSource);
  });
});
