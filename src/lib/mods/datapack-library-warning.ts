// The one warning for "this instance's data pack library could not be read".
//
// Several mounted surfaces read the same listing after the same action — the
// Add-ons tab reads it after a local install, and so does the browse view it
// embeds, whose refresh that install triggers — so one failed read would raise
// one warning per reader. This module owns the warning instead:
//   * a read that was about to open the world picker always says so, replacing
//     any vaguer warning: the user needs the reason the picker did not open;
//   * any other failed read says so unless a warning for the same instance is
//     already on screen;
//   * a successful read takes down the vaguer warning, but not the picker's —
//     that one explains something that already happened;
//   * a surface leaving the instance takes down that instance's warning.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { Error as IpcError } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { dismiss, pushWarning, toastList } from '$lib/toasts/toasts.svelte';

type Shown = { instanceId: string; id: number; blockedPicker: boolean };

let shown: Shown | null = null;

function isOnScreen(s: Shown): boolean {
  return toastList().some((toast) => toast.id === s.id);
}

function show(instanceId: string, err: IpcError, blockedPicker: boolean): void {
  if (shown !== null) dismiss(shown.id);
  const title = get(t)(
    blockedPicker
      ? 'addons.datapacks.picker.libraryReadFailed'
      : 'addons.datapacks.libraryReadFailed',
  );
  shown = { instanceId, id: pushWarning(title, [formatError(err)]), blockedPicker };
}

/** The read that would have fed the world picker failed, so it did not open. */
export function warnLibraryReadBlockedPicker(instanceId: string, err: IpcError): void {
  show(instanceId, err, true);
}

/** Any other read of the library failed. */
export function warnLibraryReadFailed(instanceId: string, err: IpcError): void {
  if (shown !== null && shown.instanceId === instanceId && isOnScreen(shown)) return;
  show(instanceId, err, false);
}

/** A read of `instanceId`'s library succeeded. */
export function libraryReadSucceeded(instanceId: string): void {
  if (shown === null || shown.instanceId !== instanceId || shown.blockedPicker) return;
  dismiss(shown.id);
  shown = null;
}

/** A surface stopped showing `instanceId` (switch or unmount). */
export function dismissLibraryReadWarning(instanceId: string): void {
  if (shown === null || shown.instanceId !== instanceId) return;
  dismiss(shown.id);
  shown = null;
}
