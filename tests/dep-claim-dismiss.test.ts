import { beforeEach, describe, expect, it } from 'vitest';
import {
  depClaimKey,
  dismissClaim,
  isClaimDismissed,
  restoreClaim,
} from '$lib/mods/dep-claim-dismiss';
import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';

// The measured pair: Status Effect Bars Reforged declares Stylish Effects
// `required` on Modrinth, while its own neoforge.mods.toml declares nothing.
const mod = { source: 'modrinth' as const, project_id: 'TxIuhIFo' };
const dep = { source: 'modrinth' as const, project_id: 'onDuQF5e' };
const other = { source: 'modrinth' as const, project_id: 'AANobbMI' };

describe('per-claim dismissal', () => {
  beforeEach(() => diagnosisDismiss.reset());

  it('hides exactly the acknowledged claim', () => {
    expect(isClaimDismissed(mod, dep)).toBe(false);
    dismissClaim(mod, dep);
    expect(isClaimDismissed(mod, dep)).toBe(true);
    // Acknowledging one claim must not blind the user to a different one on the
    // same mod — the same rule the diagnosis banners follow.
    expect(isClaimDismissed(mod, other)).toBe(false);
  });

  it('restores it', () => {
    dismissClaim(mod, dep);
    restoreClaim(mod, dep);
    expect(isClaimDismissed(mod, dep)).toBe(false);
  });

  it('keys on the pair, so the same claim is settled in every instance', () => {
    expect(depClaimKey(mod, dep)).toBe('dep:modrinth:TxIuhIFo:modrinth:onDuQF5e');
  });

  it('does not collide across sources with the same project ids', () => {
    expect(depClaimKey(mod, dep)).not.toBe(
      depClaimKey({ source: 'curseforge', project_id: 'TxIuhIFo' }, dep),
    );
  });
});
