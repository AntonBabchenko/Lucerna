import { describe, expect, it } from 'vitest';
import { tallyOptimise } from '$lib/mods/optimise-tally';

describe('tallyOptimise', () => {
  it('counts a refusal for a mod the profile already has as skipped, not failed', () => {
    const tally = tallyOptimise([
      { status: 'ok' },
      { status: 'error', error: { kind: 'mods_already_installed' } },
      { status: 'error', error: { kind: 'network' } },
      { status: 'ok' },
    ]);
    expect(tally).toEqual({ installed: 2, failed: 1, skipped: 1 });
  });

  it('a run with nothing to install comes to nothing', () => {
    expect(tallyOptimise([])).toEqual({ installed: 0, failed: 0, skipped: 0 });
  });
});
