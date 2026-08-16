import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { PreLaunchCheck } from '$lib/ipc/bindings';

const MB_PER_GB = 1024;

/** Human-readable warning lines for a pre-launch check (empty = safe to launch). */
export function warningLines(check: PreLaunchCheck): string[] {
  const translate = get(t);
  const lines: string[] = [];
  if (check.resource_warning) {
    // RAW gigabytes, never `.toFixed(1)`: the dictionary's
    // `{…, number, ::.0 group-off}` arguments own the rounding AND the decimal
    // separator, so Russian reads "14,0 ГБ" instead of an English "14.0".
    // Same rule as $lib/format/size.ts.
    lines.push(
      translate('launch.warning.ram', {
        reserved: check.resource_warning.reserved_mb / MB_PER_GB,
        total: check.resource_warning.total_mb / MB_PER_GB,
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
