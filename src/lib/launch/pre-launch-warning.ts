import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { PreLaunchCheck } from '$lib/ipc/bindings';

/** Human-readable warning lines for a pre-launch check (empty = safe to launch). */
export function warningLines(check: PreLaunchCheck): string[] {
  const translate = get(t);
  const lines: string[] = [];
  if (check.resource_warning) {
    const gb = (mb: number) => (mb / 1024).toFixed(1);
    lines.push(
      translate('launch.warning.ram', {
        reserved: gb(check.resource_warning.reserved_mb),
        total: gb(check.resource_warning.total_mb),
      }),
    );
  }
  if (check.account_conflict) {
    const key =
      check.account_conflict.account_kind === 'microsoft'
        ? 'launch.warning.accountMicrosoft'
        : 'launch.warning.accountOffline';
    lines.push(translate(key, { account: check.account_conflict.account_name }));
  }
  return lines;
}
