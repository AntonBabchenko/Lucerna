import { describe, expect, it } from 'vitest';
import { reasonLines } from '$lib/format/reason-lines';

// One grouping for every batch report — the update run, a drop's skipped files, the repair behind
// «Fix all» and «Fix and launch» (plan §5c V3): each reason once, with the names it stopped.
// Plan §5d L4: «Moonlight Lib: Не удалось связаться…» — the reason, a sentence of its own, came
// after a colon with its capital, where Russian wants a lower-case letter; lower-casing it would
// break a name it may start with («CurseForge требует…»). The names get a line of their own above
// the reason, which stays as it is.
describe('reasonLines', () => {
  it('says each reason once, with the names it stopped, in the order they came', () => {
    expect(
      reasonLines([
        { name: 'Moonlight Lib', reason: 'No connection' },
        { name: 'Sodium', reason: 'The profile is busy' },
        { name: 'ImmediatelyFast', reason: 'No connection' },
      ]),
    ).toEqual([
      { names: 'Moonlight Lib, ImmediatelyFast', reason: 'No connection' },
      { names: 'Sodium', reason: 'The profile is busy' },
    ]);
  });

  it('keeps a reason as it is, capital and all — never after a colon', () => {
    const [line] = reasonLines([
      { name: 'Moonlight Lib', reason: 'Не удалось связаться с сервером.' },
    ]);
    expect(line).toEqual({ names: 'Moonlight Lib', reason: 'Не удалось связаться с сервером.' });
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
    ).toEqual([{ names: 'a.png, a.png', reason: 'Only mod .jar files can be added here' }]);
  });
});
