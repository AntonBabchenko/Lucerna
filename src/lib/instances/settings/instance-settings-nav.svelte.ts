// A cross-component request to show the instance Settings tab, optionally at
// one field. Mirrors `modBrowserNav`: writers set it, MainTabs switches to the
// tab when the request can be delivered, and the tab consumes the field and
// clears it.
//
// `instanceId` names the instance the request is about. A request made while
// that instance's activation is still in flight must not open the tab on the
// previous instance, so it waits until that id is the active one. `null` means
// "whichever instance is active" (the Overview cards, which always show it).
import type { ManageFocusField } from '$lib/instances/manage-focus';
import { serversUi } from '$lib/servers/servers-ui.svelte';

export type InstanceSettingsField = ManageFocusField;
export type InstanceSettingsRequest = {
  instanceId: string | null;
  field: InstanceSettingsField | null;
};

export const instanceSettingsNav = $state<{ value: InstanceSettingsRequest | null }>({
  value: null,
});

/** Ask for the Settings tab. Client mode first: in servers mode the client tabs
 *  are mounted but hidden, and a flash or a focus would land out of sight. */
export function requestInstanceSettings(
  instanceId: string | null,
  field: InstanceSettingsField | null = null,
): void {
  serversUi.setMode('client');
  instanceSettingsNav.value = { instanceId, field };
}

/** Whether `req` can be delivered while `activeId` is the active instance. */
export function requestIsFor(
  req: InstanceSettingsRequest | null,
  activeId: string | null,
): boolean {
  return req !== null && (req.instanceId === null || req.instanceId === activeId);
}

/** An activation of `activatedId` finished and `activeId` is now active. A
 *  request that waited on exactly that activation and did not get it is
 *  dropped — else the tab would switch later, unasked. Any other settle leaves
 *  the request alone. */
export function settleInstanceSettingsNav(activatedId: string, activeId: string | null): void {
  const req = instanceSettingsNav.value;
  if (req?.instanceId === activatedId && activeId !== activatedId) {
    instanceSettingsNav.value = null;
  }
}

/** The user chose `id` some other way (picked it in the list, Make active). A
 *  request waiting on a different instance is no longer what they want. */
export function abandonInstanceSettingsFor(id: string): void {
  const req = instanceSettingsNav.value;
  if (req && req.instanceId !== null && req.instanceId !== id) {
    instanceSettingsNav.value = null;
  }
}

export function clearInstanceSettingsNav(): void {
  instanceSettingsNav.value = null;
}
