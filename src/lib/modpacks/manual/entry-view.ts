// Pure views of one `missing_mods` entry for the Finish-install dialog and the
// imported-pack drawer (spec 2026-11-05 §5.3): which link it opens, why it is
// there, whether it still waits for the user.

import type { Translate } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { MissingModStatus, ModpackUnresolvable, UnresolvableReason } from '$lib/ipc/bindings';
import { isUnresolvedMissingState } from '$lib/modpacks/missing-mod';

export type EntryLinkKind = 'download' | 'page' | 'link';
export type EntryLink = { url: string; kind: EntryLinkKind };

const CURSEFORGE = 'https://www.curseforge.com/';

/** The link a row opens: the file's own download page, a CurseForge page, or any other link. */
export function entryLink(e: ModpackUnresolvable): EntryLink | null {
  if (e.meta.download_url) return { url: e.meta.download_url, kind: 'download' };
  if (e.manual_action_url === '') return null;
  if (e.manual_action_url.startsWith(CURSEFORGE)) return { url: e.manual_action_url, kind: 'page' };
  return { url: e.manual_action_url, kind: 'link' };
}

export const LINK_LABEL: Record<EntryLinkKind, TranslationKey> = {
  download: 'modpacks.manual.action.download',
  page: 'modpacks.manual.action.modPage',
  link: 'modpacks.manual.action.openLink',
};

// A `Record`, so a new reason without words is a compile error.
const REASON: Record<UnresolvableReason, TranslationKey> = {
  distribution_disabled: 'modpacks.manual.reason.author',
  host_not_allowed: 'modpacks.manual.reason.host',
  unsafe_path: 'modpacks.manual.reason.unsafe',
  missing_checksum: 'modpacks.manual.reason.noChecksum',
  source_unavailable: 'modpacks.manual.reason.source',
  unsupported_type: 'modpacks.manual.reason.step',
};

export function reasonText(t: Translate, e: ModpackUnresolvable): string {
  return t(REASON[e.reason]);
}

/** Whether a published hash lets Lucerna check a file for this entry. */
export function canCheck(e: ModpackUnresolvable): boolean {
  return Boolean(e.sha1 || e.meta.md5);
}

/** Still waits for the user: the drawer count, the Overview row, the dialog's first group. */
export function needsAttention(m: MissingModStatus): boolean {
  return m.entry.reason !== 'unsupported_type' && isUnresolvedMissingState(m.state);
}
