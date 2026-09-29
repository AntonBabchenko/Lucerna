import { describe, expect, it } from 'vitest';
import { reasonLines } from '$lib/format/reason-lines';

// One grouping for every batch report — the update run, a drop's skipped files, the repair behind
// «Fix all» and «Fix and launch» (plan §5c V3): each reason once, after the names it stopped.
describe('reasonLines', () => {
  it('says each reason once, after the names it stopped, in the order they came', () => {
    expect(
      reasonLines([
        { name: 'Moonlight Lib', reason: 'No connection' },
        { name: 'Sodium', reason: 'The profile is busy' },
        { name: 'ImmediatelyFast', reason: 'No connection' },
      ]),
    ).toEqual(['Moonlight Lib, ImmediatelyFast: No connection', 'Sodium: The profile is busy']);
  });

  it('says nothing for nothing', () => {
    expect(reasonLines([])).toEqual([]);
  });

  // Two dropped files can share a name from different folders: both are listed. A caller whose
  // steps can name one mod twice for one reason drops the repeat itself (`fixAll`).
  it('keeps a name that comes twice', () => {
    expect(
      reasonLines([
        { name: 'a.png', reason: 'Only mod .jar files can be added here' },
        { name: 'a.png', reason: 'Only mod .jar files can be added here' },
      ]),
    ).toEqual(['a.png, a.png: Only mod .jar files can be added here']);
  });
});
