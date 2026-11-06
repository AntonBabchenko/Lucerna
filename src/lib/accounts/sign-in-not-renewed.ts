import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { Error as IpcError, SignInNotRenewed } from '$lib/ipc/bindings';

/** The warning toast a `sign-in-not-renewed` event shows. */
export type SignInNotRenewedToast = {
  titleKey: TranslationKey;
  /** The account's player name, for the title's `{name}`. */
  name: string;
  /** The reason as `formatError` words it, shown first; null when the title already says it. */
  cause: IpcError | null;
  lineKeys: TranslationKey[];
  /** Offer «Sign in again» — only when signing in again is all that can help: Microsoft refused the
   *  saved sign-in, or none is stored (`MicrosoftSignInExpired`). */
  signInAgain: boolean;
};

/**
 * Map the event to its toast. Pure — the page translates, formats the cause and
 * wires «Sign in again» to the sidebar's own sign-in flow.
 */
export function signInNotRenewedToast(event: SignInNotRenewed): SignInNotRenewedToast {
  if (event.error.kind === 'microsoft_sign_in_expired') {
    return {
      titleKey: 'page.accounts.notRenewedExpiredTitle',
      name: event.account_name,
      cause: null,
      lineKeys: ['page.accounts.notRenewedStarted'],
      signInAgain: true,
    };
  }
  // Offline, Microsoft down, an Xbox refusal, a keyring that cannot be read:
  // signing in again is not known to fix any of these, so it is not offered.
  return {
    titleKey: 'page.accounts.notRenewedTitle',
    name: event.account_name,
    cause: event.error,
    lineKeys: ['page.accounts.notRenewedStarted', 'page.accounts.notRenewedRetry'],
    signInAgain: false,
  };
}
