import { describe, expect, it } from 'vitest';
import {
  CHANGELOG_LOCALES,
  CHANGELOG_SOURCE_LOCALE,
  loadChangelogLocale,
} from '$lib/changelog/locales';

describe('changelog locale files', () => {
  it('discovers ru from src/lib/changelog/locales and never lists the source language', () => {
    expect(CHANGELOG_SOURCE_LOCALE).toBe('en');
    expect(CHANGELOG_LOCALES).toContain('ru');
    expect(CHANGELOG_LOCALES).not.toContain('en');
  });

  it('loads and parses the Russian changelog into versions with content', async () => {
    const r = await loadChangelogLocale('ru');
    expect(r.status).toBe('ready');
    if (r.status !== 'ready') return;
    expect(r.entries.length).toBeGreaterThan(0);
    expect(r.entries.some((v) => v.sections.length > 0)).toBe(true);
  });

  it('reports a locale with no file as missing', async () => {
    expect(await loadChangelogLocale('xx')).toEqual({ status: 'missing' });
  });

  it('reports a loader that throws as failed, not as missing', async () => {
    const boom = new Error('boom');
    const r = await loadChangelogLocale('de', { de: () => Promise.reject(boom) });
    expect(r).toEqual({ status: 'failed', error: boom });
  });

  it('treats a file that parses to nothing as failed (unusable), not as ready', async () => {
    const r = await loadChangelogLocale('de', { de: () => Promise.resolve('# nothing here') });
    expect(r.status).toBe('failed');
  });
});
