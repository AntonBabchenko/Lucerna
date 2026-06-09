import type { TranslationKey } from '$lib/i18n/keys.generated';

/**
 * Official Minecraft Java/Bedrock store page. Opened (via the system browser)
 * when a Microsoft account signs in successfully but does not own Minecraft,
 * giving the user a direct route to purchase instead of a dead-end error.
 */
export const MINECRAFT_BUY_URL =
  'https://www.minecraft.net/store/minecraft-java-bedrock-edition-pc';

/** A toast descriptor derived from a Microsoft sign-in failure. */
export type SignInErrorToast = {
  kind: 'info' | 'warning';
  titleKey: TranslationKey;
  /** When set, the toast should offer a "Buy Minecraft" action opening this URL. */
  buyLink?: string;
};

/**
 * Map a Microsoft sign-in error (the typed `Error` from the IPC layer) to the
 * toast that should be shown. Pure — the caller owns translation and the
 * `openUrl` side effect so this stays trivially testable.
 */
export function classifySignInError(err: unknown): SignInErrorToast {
  const kind =
    typeof err === 'object' && err !== null ? (err as { kind?: string }).kind : undefined;

  switch (kind) {
    case 'auth_pending_approval':
      return { kind: 'info', titleKey: 'page.accounts.pendingApproval' };
    case 'no_minecraft_profile':
      return {
        kind: 'warning',
        titleKey: 'page.accounts.noMinecraftTitle',
        buyLink: MINECRAFT_BUY_URL,
      };
    default:
      return { kind: 'warning', titleKey: 'page.accounts.signInFailed' };
  }
}
