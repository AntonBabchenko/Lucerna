// tests/properties-catalog.test.ts
import { describe, expect, it } from 'vitest';
import en from '$lib/i18n/locales/en.json';
import ru from '$lib/i18n/locales/ru.json';
import {
  keyToI18n,
  PROP_GROUP_ORDER,
  PROPERTIES_CATALOG,
  type PropSpec,
} from '$lib/servers/settings/properties-catalog';

describe('properties catalog', () => {
  it('has no duplicate keys', () => {
    const keys = PROPERTIES_CATALOG.map((s) => s.key);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it('every group is in the render order', () => {
    for (const s of PROPERTIES_CATALOG) {
      expect(PROP_GROUP_ORDER).toContain(s.group);
    }
  });

  it('each default is consistent with its type', () => {
    for (const s of PROPERTIES_CATALOG) {
      assertDefaultValid(s);
    }
  });

  it('keyToI18n camelCases separators', () => {
    expect(keyToI18n('server-port')).toBe('serverPort');
    expect(keyToI18n('rcon.port')).toBe('rconPort');
    expect(keyToI18n('resource-pack-sha1')).toBe('resourcePackSha1');
  });
});

function assertDefaultValid(s: PropSpec): void {
  const d = s.default;
  switch (s.type.kind) {
    case 'bool':
      expect(['true', 'false'], s.key).toContain(d);
      break;
    case 'enum':
      if (d !== '') expect(s.type.values, s.key).toContain(d);
      break;
    case 'int':
      if (d !== '') expect(Number.isInteger(Number(d)), `${s.key}=${d}`).toBe(true);
      break;
    case 'string':
      expect(typeof d, s.key).toBe('string');
      break;
  }
}

function leaf(obj: unknown, path: string): unknown {
  return path.split('.').reduce<unknown>((o, k) => (o as Record<string, unknown>)?.[k], obj);
}

describe('properties catalog i18n', () => {
  for (const locale of [
    { name: 'en', dict: en },
    { name: 'ru', dict: ru },
  ]) {
    it(`${locale.name} has label+desc for every key`, () => {
      for (const s of PROPERTIES_CATALOG) {
        const base = `servers.props.${keyToI18n(s.key)}`;
        expect(leaf(locale.dict, `${base}.label`), `${locale.name} ${base}.label`).toBeTruthy();
        expect(leaf(locale.dict, `${base}.desc`), `${locale.name} ${base}.desc`).toBeTruthy();
      }
    });

    it(`${locale.name} has every group title`, () => {
      for (const g of PROP_GROUP_ORDER) {
        expect(leaf(locale.dict, `servers.propGroups.${g}`), `${locale.name} ${g}`).toBeTruthy();
      }
    });
  }
});
