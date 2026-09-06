import { describe, expect, it } from 'vitest';
import { KIND_LABEL_KEY, ORIGIN_LABEL_KEY, TASK_KINDS } from '$lib/tasks/types';

describe('task label maps', () => {
  it('has a label key for every task kind', () => {
    for (const kind of TASK_KINDS) expect(KIND_LABEL_KEY[kind]).toBeTruthy();
  });

  it('has a label key for every detail origin', () => {
    const origins = ['modrinth', 'curseforge', 'ftb', 'atlauncher', 'archive', 'local'] as const;
    for (const o of origins) expect(ORIGIN_LABEL_KEY[o]).toBeTruthy();
  });

  it('registers world-migrate as a task kind with its own label key', () => {
    expect(TASK_KINDS).toContain('world-migrate');
    expect(KIND_LABEL_KEY['world-migrate']).toBe('tasks.kind.worldMigrate');
  });
});
