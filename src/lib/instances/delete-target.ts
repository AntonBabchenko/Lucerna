import type { InstanceWithStatus } from '$lib/ipc/bindings';

// The instance a delete confirm is about, taken when the confirm opens. The
// delete refreshes the instance list before the dialog reports back, and the
// deleted instance is not in the new list: a target looked up in the list would
// be gone by then, with the dialog and its callbacks.
//
// The list, else the active instance (the list read can fail while the active
// one is known, and the Settings tab still offers Delete then). Null only for
// an id neither knows; every caller offers ids taken from one of the two.
export function findDeleteTarget(
  id: string,
  instances: readonly InstanceWithStatus[],
  active: InstanceWithStatus | null,
): InstanceWithStatus | null {
  return instances.find((i) => i.id === id) ?? (active?.id === id ? active : null);
}

// After the delete: the name of the instance the backend made active in place
// of the deleted one, or null when there is nothing to say. `activeBefore` is
// the active id as the delete returned, before the refresh: not the one when
// the confirm opened, since a toast action or a shortcut can switch the active
// instance while it is open.
export function nowActiveAfterDelete(
  deletedId: string,
  activeBefore: string | null,
  activeAfter: InstanceWithStatus | null,
): string | null {
  if (activeBefore !== deletedId || !activeAfter || activeAfter.id === deletedId) return null;
  return activeAfter.name;
}
