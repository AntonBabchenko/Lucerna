import { describe, expect, test } from 'vitest';
import type { AnnotateResult } from '$lib/ipc/bindings';
import { DIAGNOSIS_COPY } from '$lib/logs/diagnosis-copy';
import { annotationMap, fallbackCopyMap, resolveHintCopy } from '$lib/logs/log-hints.svelte';
import en from '../src/lib/i18n/locales/en.json';
import ru from '../src/lib/i18n/locales/ru.json';

const RESULT: AnnotateResult = {
  annotations: [
    { line: 3, pattern_id: 'oom-gc-overhead' },
    { line: 7, pattern_id: 'dns-unknown-host' },
  ],
  patterns: [
    {
      id: 'oom-gc-overhead',
      title: 'Memory is so full Java gave up',
      explanation: 'e',
      recommendation: 'r',
    },
    { id: 'dns-unknown-host', title: 't', explanation: 'e', recommendation: 'r' },
  ],
};

describe('annotation mapping', () => {
  test('annotationMap keys line index to pattern id', () => {
    const map = annotationMap(RESULT);
    expect(map.get(3)).toBe('oom-gc-overhead');
    expect(map.get(7)).toBe('dns-unknown-host');
    expect(map.size).toBe(2);
  });

  test('annotationMap of null result is empty', () => {
    expect(annotationMap(null).size).toBe(0);
  });

  test('resolveHintCopy prefers localized copy for known ids', () => {
    const copy = resolveHintCopy('oom-gc-overhead', fallbackCopyMap(RESULT), (k) => `L:${k}`);
    expect(copy?.title).toBe('L:logs.diagnosis.patterns.oomGcOverhead.title');
  });

  test('resolveHintCopy falls back to backend English for unknown ids', () => {
    const result: AnnotateResult = {
      annotations: [{ line: 0, pattern_id: 'future-pattern' }],
      patterns: [{ id: 'future-pattern', title: 'FT', explanation: 'FE', recommendation: 'FR' }],
    };
    const copy = resolveHintCopy('future-pattern', fallbackCopyMap(result), (k) => `L:${k}`);
    expect(copy).toEqual({ title: 'FT', explanation: 'FE', recommendation: 'FR' });
  });

  test('resolveHintCopy returns null when nothing is known', () => {
    expect(resolveHintCopy('ghost', new Map(), (k) => k)).toBeNull();
  });
});

describe('locale coverage for hint copy', () => {
  // Every id mapped in DIAGNOSIS_COPY must have real strings in BOTH
  // locales — this is what keeps ru.json honest when the catalog grows.
  const patternIds = Object.keys(DIAGNOSIS_COPY);

  function resolve(tree: Record<string, unknown>, dotPath: string): unknown {
    return dotPath.split('.').reduce<unknown>((node, part) => {
      if (node && typeof node === 'object') return (node as Record<string, unknown>)[part];
      return undefined;
    }, tree);
  }

  test.each(patternIds)('%s has en + ru copy', (id) => {
    const keys = DIAGNOSIS_COPY[id];
    for (const key of [keys.title, keys.explanation, keys.recommendation]) {
      const enVal = resolve(en as Record<string, unknown>, key);
      const ruVal = resolve(ru as Record<string, unknown>, key);
      expect(typeof enVal, `${key} missing in en.json`).toBe('string');
      expect(typeof ruVal, `${key} missing in ru.json`).toBe('string');
      expect((enVal as string).length).toBeGreaterThan(0);
      expect((ruVal as string).length).toBeGreaterThan(0);
    }
  });
});
