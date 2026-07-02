import { get } from 'svelte/store';
import { locale, type Translate } from '$lib/i18n';
import type { LastUpload, UploadPreflight } from '$lib/ipc/bindings';

/** The fields of `UploadPreflight` the UI reasons about (mirrors the binding). */
export type PreflightView = Pick<UploadPreflight, 'total_bytes' | 'free_bytes' | 'exceeds_free'>;

/** Severity of a preflight result: fits, over capacity, or free space unknown. */
export type PreflightSeverity = 'ok' | 'over' | 'unknown';

/**
 * Build the "Последняя заливка: <when> → <target>" line, or null when the server
 * has never had a successful upload. `<when>` is the locale's date+time string;
 * the i18n template owns the surrounding copy and the arrow.
 */
export function formatLastUpload(t: Translate, last: LastUpload | null | undefined): string | null {
  if (!last) return null;
  // `unix_ms` is typed nullable on the wire (specta `f64`); the Rust side always
  // writes a finite ms count, but coerce null → epoch 0 to satisfy `Date`.
  // Format against the APP locale (svelte-i18n's `locale` store), not the OS
  // locale, so the date matches the rest of the UI's language — the Backups
  // dialogs use the same `$locale` pattern.
  const when = new Date(last.unix_ms ?? 0).toLocaleString(get(locale) ?? undefined);
  return t('servers.hosting.lastUpload', { when, target: last.target });
}

/**
 * Classify a preflight: `over` (known free space exceeded → warn), `unknown`
 * (server reported no free space → show total only), or `ok` (fits).
 */
export function preflightLevel(p: PreflightView): PreflightSeverity {
  if (p.free_bytes == null) return 'unknown';
  return p.exceeds_free ? 'over' : 'ok';
}
