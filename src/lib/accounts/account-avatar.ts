/** A network-free fallback avatar: an initial tinted by a name-derived hue. */
export interface AccountAvatar {
  letter: string;
  hue: number;
}

/**
 * Derive a deterministic letter avatar from an account name: the uppercased
 * first codepoint, tinted by a stable hue hashed from the full name. Used
 * when no skin is available (offline accounts, fetch failure, default skin).
 */
export function deriveAccountAvatar(name: string): AccountAvatar {
  const first = Array.from(name.trimStart())[0];
  const letter = first ? first.toUpperCase() : '?';
  let hash = 0;
  for (const ch of name) {
    hash = (hash * 31 + (ch.codePointAt(0) ?? 0)) >>> 0;
  }
  return { letter, hue: hash % 360 };
}
