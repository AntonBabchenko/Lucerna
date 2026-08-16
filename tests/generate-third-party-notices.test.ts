import { describe, expect, test } from 'vitest';
import {
  composeNotices,
  composeNpmSection,
  parsePnpmLicenses,
} from '../tools/generate-third-party-notices.mjs';

describe('generate-third-party-notices', () => {
  test('parses pnpm licenses JSON into a sorted flat package list', () => {
    const json = JSON.stringify({
      MIT: [
        { name: 'zeta', versions: ['1.0.0'], paths: ['/p/zeta'], license: 'MIT' },
        { name: 'alpha', version: '2.0.0', path: '/p/alpha', license: 'MIT' },
      ],
      ISC: [{ name: 'mid', versions: ['3.1.4'], paths: ['/p/mid'], license: 'ISC' }],
    });

    const packages = parsePnpmLicenses(json);

    expect(packages.map((p) => p.name)).toEqual(['alpha', 'mid', 'zeta']);
    // singular fields (older pnpm shape) are normalized to arrays
    expect(packages[0].versions).toEqual(['2.0.0']);
    expect(packages[0].paths).toEqual(['/p/alpha']);
  });

  test('falls back to the manifest license when a package ships no license file', () => {
    const json = JSON.stringify({
      MIT: [{ name: 'nofile', versions: ['1.0.0'], paths: ['/p/nofile'], license: 'MIT' }],
    });

    const packages = parsePnpmLicenses(json);
    const section = composeNpmSection(packages, () => []);

    expect(section).toContain('nofile 1.0.0');
    expect(section).toContain('ships no license file');
  });

  test('composed file names both halves and the regeneration command', () => {
    const out = composeNotices('NPM-HALF', 'RUST-HALF');

    expect(out).toContain('Frontend (npm) components');
    expect(out).toContain('Backend (Rust crate) components');
    expect(out).toContain('NPM-HALF');
    expect(out).toContain('RUST-HALF');
    expect(out).toContain('tools/generate-third-party-notices.mjs');
  });
});
