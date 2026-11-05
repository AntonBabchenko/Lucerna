import { describe, expect, it } from 'vitest';
import type { SupplyRow } from '$lib/ipc/bindings';
import { supplyResult } from '$lib/modpacks/manual/supply-lines';

const t = ((key: string, args?: Record<string, unknown>) =>
  args ? `${key} ${JSON.stringify(args)}` : key) as never;

describe('supplyResult', () => {
  it('counts what was added and groups refusals by reason', () => {
    const rows: SupplyRow[] = [
      { kind: 'added', path: 'C:/dl/a.jar', key: 'file:1', mod_name: 'A', recorded: true },
      { kind: 'not_in_pack', path: 'C:/dl/b.jar', found: null, asked: true },
      { kind: 'not_in_pack', path: 'C:/dl/c.jar', found: null, asked: true },
      { kind: 'other_version', path: 'C:/dl/d.jar', found: 'SRP 1.2.0', wanted: 'srp-1.1.0.jar' },
    ];
    const r = supplyResult(t, rows);
    expect(r.message).toContain('modpacks.manual.result.added');
    expect(r.tone).toBe('warning');
    expect(r.details).toEqual([
      { names: 'b.jar, c.jar', reason: 'modpacks.manual.result.notInPack' },
      {
        names: 'd.jar',
        reason:
          'modpacks.manual.result.otherVersion {"found":"SRP 1.2.0","wanted":"srp-1.1.0.jar"}',
      },
    ]);
  });
  it('is quiet success when everything landed', () => {
    const r = supplyResult(t, [
      { kind: 'added', path: 'a.jar', key: 'k', mod_name: 'A', recorded: true },
    ]);
    expect(r.tone).toBe('success');
    expect(r.details).toEqual([]);
  });
  it('names a refused file by its name on a Windows path, not by the whole path', () => {
    const r = supplyResult(t, [{ kind: 'wrong_type', path: 'C:\\Users\\u\\Downloads\\notes.txt' }]);
    expect(r.details).toEqual([{ names: 'notes.txt', reason: 'modpacks.manual.result.wrongType' }]);
  });
});
