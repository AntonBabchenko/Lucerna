// The `datapack-load-failed` hint's copy (spec 2026-09-24 §4 U6, A11). It must
// not blame versions (an out-of-range pack_format is never refused), must point
// ABOVE the matched line (the game names the file in the ERROR block before
// it), must name both owners of a namespace, and must keep the Safe Mode caveat
// (a world saved in Safe Mode clears Disabled).
import { describe, expect, it } from 'vitest';
import en from '../src/lib/i18n/locales/en.json';
import ru from '../src/lib/i18n/locales/ru.json';

const EN = en.logs.diagnosis.patterns.datapackLoadFailed;
const RU = ru.logs.diagnosis.patterns.datapackLoadFailed;

describe('datapack-load-failed copy', () => {
  it("EN+RU copy names the Safe Mode caveat and doesn't blame versions", () => {
    expect(EN.explanation).not.toMatch(/different Minecraft version/);
    expect(EN.recommendation).toMatch(/forgets which packs/);
    expect(RU.recommendation).toMatch(/забывает/);
  });

  it('points above the matched line, where the game names the file', () => {
    for (const s of [EN.explanation, EN.recommendation]) {
      expect(s).toMatch(/above/);
      expect(s).not.toMatch(/below/);
    }
    for (const s of [RU.explanation, RU.recommendation]) {
      expect(s).toMatch(/выше|над этой/);
      expect(s).not.toMatch(/ниже|под этой/);
    }
  });

  it('names both owners of a namespace: a data pack or a mod', () => {
    expect(EN.recommendation).toMatch(/if a mod owns it/i);
    expect(RU.recommendation).toMatch(/если моду/);
  });
});
