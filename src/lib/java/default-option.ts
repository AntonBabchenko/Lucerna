// The «Default» option of an instance's or a server's Java field: what it
// follows today. Settings' row for the Java major the version needs, else
// Lucerna's runtime (spec 2026-10-11-custom-java §6.2). Reads the displayed
// Settings block, so the label follows a row saved in Settings at once.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { JavaDefault } from '$lib/ipc/bindings';
import { generalDisplayed } from '$lib/settings/app-settings.svelte';
import type { DefaultOption } from './JavaChoiceField.svelte';

/** What Settings says for a Java major: its row, none, or — the need not
 *  known yet, or Settings not read (pending or unreadable) — unknown, which is
 *  never shown as «Lucerna's Java». */
export type SettingsRow =
  | { kind: 'unknown' }
  | { kind: 'none' }
  | { kind: 'row'; row: JavaDefault };

export function settingsRowFor(required: number | null): SettingsRow {
  const general = generalDisplayed();
  if (required === null || general === null) return { kind: 'unknown' };
  const row = general.java_defaults?.find((d) => d.major === required);
  return row ? { kind: 'row', row } : { kind: 'none' };
}

export function defaultOptionFor(owner: 'instance' | 'server'): DefaultOption {
  return {
    labelFor: (required) => {
      const tr = get(t);
      const answer = settingsRowFor(required);
      if (answer.kind === 'unknown' || required === null) return tr('java.default.label');
      return answer.kind === 'row'
        ? tr('java.default.labelSettings', { major: required })
        : tr('java.default.labelLucerna');
    },
    statusFor: (required) => {
      const tr = get(t);
      if (required === null) {
        return tr(
          owner === 'instance'
            ? 'java.default.statusUnknownInstance'
            : 'java.default.statusUnknownServer',
        );
      }
      const answer = settingsRowFor(required);
      if (answer.kind === 'unknown') return tr('java.default.statusUnread', { major: required });
      return answer.kind === 'row'
        ? tr('java.default.statusSettings', { major: required, path: answer.row.path })
        : tr('instance.manage.java.statusBundled');
    },
  };
}
