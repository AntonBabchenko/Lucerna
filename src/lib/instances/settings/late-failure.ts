// A field that saves when it is left can finish after the user left the
// Settings tab or switched instances: a click on another tab blurs the field
// first and unmounts the tab right after. The write still lands (the caller
// refreshes the list), but the section that would show its error is gone — so
// a failure is said in a toast, naming the instance and the field.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { pushWarning } from '$lib/toasts/toasts.svelte';

export function reportLateFailure(instanceName: string, field: string, message: string): void {
  pushWarning(get(t)('instance.settings.saveFailedLate', { name: instanceName, field }), [
    message,
  ]);
}
