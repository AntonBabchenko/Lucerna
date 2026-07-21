import { describe, expect, it } from 'vitest';
import type { ModSummary } from '$lib/ipc/bindings';
import { enrichedToCard } from '$lib/servers/addons/server-card-adapter';

const summary: ModSummary = {
  source: 'modrinth',
  project_id: 'pid',
  slug: 'sodium',
  name: 'Sodium',
  summary: '',
  icon_url: 'http://x/i.png',
  downloads: 1,
  author: 'a',
  updated_at: null,
};
const base = { on_disk_filename: '', sha1: 'x' };

describe('enrichedToCard', () => {
  it('inverts disabled→enabled and leaves summary null without identity', () => {
    const row = enrichedToCard(
      {
        ...base,
        filename: 'foo.jar',
        on_disk_filename: 'foo.jar.disabled',
        disabled: true,
        sha1: 'ab',
        source: null,
        project_id: null,
        version_id: null,
        name: null,
        version_number: null,
      },
      new Map(),
    );
    expect(row.installed.enabled).toBe(false);
    expect(row.installed.name).toBe('foo.jar');
    expect(row.summary).toBeNull();
  });
  it('attaches the resolved summary when identity is present', () => {
    const byKey = new Map([['modrinth:pid', summary]]);
    const row = enrichedToCard(
      {
        ...base,
        filename: 'sodium.jar',
        on_disk_filename: 'sodium.jar',
        disabled: false,
        sha1: 'cd',
        source: 'modrinth',
        project_id: 'pid',
        version_id: 'v',
        name: 'Sodium',
        version_number: '0.5',
      },
      byKey,
    );
    expect(row.installed.enabled).toBe(true);
    expect(row.summary?.name).toBe('Sodium');
    expect(row.installed.version_number).toBe('0.5');
  });
});
