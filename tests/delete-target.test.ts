/**
 * The page's delete confirm is about one instance, taken when it opens. The
 * delete itself refreshes the instance list, and the deleted instance is not in
 * it any more: anything the page derived from the list would vanish before the
 * dialog reported back (the live check found no "now active" toast and focus on
 * the body). These pin what is taken and what is said afterwards.
 */
import { describe, expect, it } from 'vitest';
import { findDeleteTarget, nowActiveAfterDelete } from '$lib/instances/delete-target';
import type { InstanceWithStatus } from '$lib/ipc/bindings';

function inst(id: string, name = id): InstanceWithStatus {
  return { id, name } as InstanceWithStatus;
}

const a = inst('a', 'Alpha');
const b = inst('b', 'Beta');

describe('findDeleteTarget', () => {
  it('takes the instance from the list', () => {
    expect(findDeleteTarget('b', [a, b], a)).toBe(b);
  });

  it('takes the active instance when the list could not be read', () => {
    expect(findDeleteTarget('a', [], a)).toBe(a);
  });

  it('is nothing for an id neither the list nor the active instance knows', () => {
    expect(findDeleteTarget('zz', [a, b], a)).toBeNull();
    expect(findDeleteTarget('zz', [], null)).toBeNull();
  });
});

describe('nowActiveAfterDelete', () => {
  it('names the instance the backend made active when the active one was deleted', () => {
    expect(nowActiveAfterDelete('a', 'a', b)).toBe('Beta');
  });

  it('says nothing when another instance was deleted', () => {
    expect(nowActiveAfterDelete('b', 'a', a)).toBeNull();
  });

  // Decided by what was active as the delete returned, not when the confirm
  // opened: a toast action or a shortcut can switch while it is open.
  it('says nothing when the deleted one stopped being active before the delete', () => {
    expect(nowActiveAfterDelete('a', 'b', b)).toBeNull();
  });

  it('says nothing when no instance is active afterwards', () => {
    expect(nowActiveAfterDelete('a', 'a', null)).toBeNull();
  });

  it('says nothing when the refresh still reports the deleted instance as active', () => {
    expect(nowActiveAfterDelete('a', 'a', a)).toBeNull();
  });
});
