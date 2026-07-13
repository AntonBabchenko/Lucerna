import { describe, expect, it } from 'vitest';
import { groupProjectIdsBySource } from '$lib/servers/addons/server-installed-data.svelte';

describe('groupProjectIdsBySource', () => {
  it('groups project_ids by source, skipping identity-less rows', () => {
    const map = groupProjectIdsBySource([
      { source: 'modrinth', project_id: 'a' },
      { source: 'modrinth', project_id: 'b' },
      { source: 'curseforge', project_id: 'c' },
      { source: null, project_id: null },
    ]);
    expect([...(map.get('modrinth') ?? [])].sort()).toEqual(['a', 'b']);
    expect([...(map.get('curseforge') ?? [])]).toEqual(['c']);
  });
});
