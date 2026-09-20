import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// D8 leg 3. The chip's live verdicts never expire within a platform triple;
// the migration plan is fetched fresh on every open. The view re-runs the
// chip's check once the plan has loaded, so the two are never compared across
// a stale store. Behaviourally covered at the dialog (`onPlanLoaded` fires
// once, on success); this pins the ONE line that connects it.
describe('plan open re-checks the chip', () => {
  it('InstalledModsView passes runLiveCheck as onPlanLoaded', () => {
    const body = readFileSync(
      join(import.meta.dirname, '../src/lib/mods/installed/InstalledModsView.svelte'),
      'utf8',
    );
    expect(body).toContain('onPlanLoaded={() => void compat.runLiveCheck()}');
  });
});
