import type { OfflineNameRejection } from '$lib/ipc/bindings';

/**
 * Offline nickname rule, mirrored from the Rust source of truth
 * (`src-tauri/src/accounts/offline_name.rs`). Minecraft offline play only
 * accepts ASCII `[A-Za-z0-9_]`, 3–16 characters; other names (e.g. Cyrillic)
 * can't enter a world. Keep this in sync with the backend.
 */
export const OFFLINE_NAME_MIN = 3;
export const OFFLINE_NAME_MAX = 16;

const OFFLINE_NAME_CHARSET = /^[A-Za-z0-9_]+$/;

/**
 * Validate an already-trimmed offline nickname. Returns `null` when valid, or
 * the rejection reason (matching the backend's typed reason). Length is checked
 * before charset, and length counts Unicode code points (parity with Rust's
 * `chars().count()`).
 */
export function validateOfflineName(name: string): OfflineNameRejection | null {
  const len = Array.from(name).length;
  if (len < OFFLINE_NAME_MIN) return 'too_short';
  if (len > OFFLINE_NAME_MAX) return 'too_long';
  if (!OFFLINE_NAME_CHARSET.test(name)) return 'invalid_chars';
  return null;
}

/** i18n key for a rejection reason. Shared by the modal hint and `formatError`. */
export function offlineNameRejectionKey(
  reason: OfflineNameRejection,
): 'page.accounts.offlineNameTooShort' | 'page.accounts.offlineNameTooLong' | 'page.accounts.offlineNameInvalidChars' {
  switch (reason) {
    case 'too_short':
      return 'page.accounts.offlineNameTooShort';
    case 'too_long':
      return 'page.accounts.offlineNameTooLong';
    case 'invalid_chars':
      return 'page.accounts.offlineNameInvalidChars';
  }
}
