/**
 * `+page.svelte` hosts the instance Settings tab, the create and delete dialogs
 * that replaced the Manage window, and the bookkeeping of a Settings request.
 * The page is the whole app shell and is not renderable under vitest, so — like
 * `tests/page-preflight-wiring.test.ts` — these are source-scan guards. They
 * prove the wiring exists; the behaviour is pinned by the components' own
 * suites and `tests/instance-settings-nav.test.ts`.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const src = readFileSync(resolve('src/routes/+page.svelte'), 'utf8');

function functionBody(header: string): string {
  const start = src.indexOf(header);
  expect(start, `${header} must exist in +page.svelte`).toBeGreaterThan(-1);
  const open = src.indexOf('{', start);
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    if (src[i] === '{') depth++;
    else if (src[i] === '}' && --depth === 0) return src.slice(open, i + 1);
  }
  throw new Error(`unbalanced braces after ${header}`);
}

describe('+page — the instance Settings tab', () => {
  it('no longer mounts the Manage window', () => {
    expect(src).not.toContain('ManageInstancesModal');
    expect(src).not.toContain('manageOpen');
  });

  it('renders the tab through MainTabs’ settings snippet, with the list-read error', () => {
    expect(src).toContain('{#snippet settings()}');
    expect(src).toContain('<InstanceSettingsTab');
    expect(src).toContain('listError={instancesListError}');
  });

  it('hosts the create and delete dialogs', () => {
    expect(src).toContain('<CreateInstanceDialog');
    expect(src).toContain('<DeleteInstanceDialog');
  });

  it('expands the compact window for the dialogs the sidebar can now open', () => {
    const effect = src.slice(
      src.indexOf('const anyWideOverlay ='),
      src.indexOf('const anyWideOverlay =') + 600,
    );
    for (const term of [
      'createInstanceOpen',
      'deleteTarget !== null',
      'l10nOpen',
      'instanceSettingsNav.value !== null',
    ]) {
      expect(effect).toContain(term);
    }
    // A refused expand drops a waiting Settings request.
    expect(src).toContain('if (compactState.value) clearInstanceSettingsNav();');
  });

  it('takes the delete target when the confirm opens, not from the refreshed list', () => {
    // The delete refreshes the list before the dialog reports back; a target
    // looked up in it would be gone, with the dialog and the "now active" toast.
    expect(src).not.toContain('instances.find((i) => i.id === deleteTarget');
    expect(src).toContain('findDeleteTarget(');
    expect(src).toContain('instance={target}');
    // Whether the deleted one was active is read as the delete returns.
    expect(src).toContain('refresh={refreshAfterDelete}');
    expect(functionBody('async function refreshAfterDelete(')).toContain(
      'activeBeforeDelete = activeInstance?.id ?? null;',
    );
    expect(functionBody('async function afterDelete(')).toContain('nowActiveAfterDelete(');
  });

  it('settles a waiting Settings request after every activation', () => {
    const body = functionBody('async function activateInstance(');
    expect(body).toContain('abandonInstanceSettingsFor(id)');
    expect(body.split('settleInstanceSettingsNav(').length - 1).toBe(2);
  });

  it('lets only the newest instance-list read land (latest-read.test.ts pins how)', () => {
    expect(src).toContain('const refreshInstances = latestRead(');
  });

  it('a failed pick from the sidebar is a toast, not only the Overview banner', () => {
    const body = functionBody('async function onSelectInstance(');
    expect(body).toContain('reportActivationFailure(');
  });
});
