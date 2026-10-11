// The «Default» option of an instance's or a server's Java field: what it
// follows today. Settings' row for the Java major the version needs, else
// Lucerna's runtime (spec 2026-10-11-custom-java §6.2). Reads the displayed
// Settings block, so the label follows a row saved in Settings at once.
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { JavaDefault } from '$lib/ipc/bindings';
import { generalDisplayed } from '$lib/settings/app-settings.svelte';
import type { DefaultOption } from './JavaChoiceField.svelte';

/** Settings' row for a Java major, when there is one. */
export function settingsRowFor(required: number | null): JavaDefault | null {
  if (required === null) return null;
  return generalDisplayed()?.java_defaults?.find((d) => d.major === required) ?? null;
}

export function defaultOptionFor(owner: 'instance' | 'server'): DefaultOption {
  return {
    labelFor: (required) => {
      const tr = get(t);
      if (required === null) return tr('java.default.label');
      return settingsRowFor(required)
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
      const row = settingsRowFor(required);
      return row
        ? tr('java.default.statusSettings', { major: required, path: row.path })
        : tr('instance.manage.java.statusBundled');
    },
  };
}
