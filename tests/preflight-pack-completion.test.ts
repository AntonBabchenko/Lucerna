/**
 * A pack that ships a completer mod arrives with real unmet mandatory
 * dependencies, by design — it fetches the rest on first launch. Blocking that
 * launch demands a decision where there is nothing to decide, and the fix is
 * only reachable by launching past our own warning.
 *
 * The same report blocks once the last outstanding file has landed, with no
 * state to reset.
 */
import { describe, expect, it } from 'vitest';
import type { PreflightReport } from '$lib/ipc/bindings';
import { hasBlocking } from '$lib/mods/preflight.svelte';

const violation = {
  dependent_sha1: 'a',
  dependent_name: 'Alpha',
  dep_id: 'ftbquests',
  dep_display_name: null,
  kind: 'missing_required',
  installed_version: null,
  needed: '',
  needed_desc: { raw: '', family: 'maven', alternatives: [], unparseable: false, soft: false },
  provider_project: null,
  provider_sha1: null,
  family: null,
} as unknown as PreflightReport['violations'][number];

const pending = {
  display_name: 'FTB Quests',
  pattern: 'ftb-quests-fabric-2001.4.17.jar',
  url: 'https://example.invalid/x',
  destination: 'mods',
};

describe('a self-completing pack does not block the launch', () => {
  it('blocks when there is no completer at all', () => {
    expect(hasBlocking({ violations: [violation], pack_completion: null })).toBe(true);
  });

  it('does not block while the pack is still waiting for files', () => {
    expect(
      hasBlocking({
        violations: [violation],
        pack_completion: { total: 34, outstanding: [pending] },
      }),
    ).toBe(false);
  });

  it('blocks again once every file has landed', () => {
    expect(
      hasBlocking({ violations: [violation], pack_completion: { total: 34, outstanding: [] } }),
    ).toBe(true);
  });

  it('never blocks with no violations, completer or not', () => {
    expect(hasBlocking({ violations: [], pack_completion: null })).toBe(false);
    expect(
      hasBlocking({ violations: [], pack_completion: { total: 1, outstanding: [pending] } }),
    ).toBe(false);
  });
});
