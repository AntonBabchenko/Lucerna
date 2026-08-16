#!/usr/bin/env node
// Generates THIRD-PARTY-NOTICES.txt at the repo root — the licenses of every
// third-party component that ships inside a Lucerna release binary.
//
// Two halves:
//   npm  — production dependency tree, read from `pnpm licenses list --prod
//          --json` plus each package's own LICENSE/COPYING/NOTICE file text.
//   Rust — delegated to `cargo about generate` (config: src-tauri/about.toml,
//          template: src-tauri/about.hbs), which resolves every crate in the
//          shipped dependency graph and deduplicates license texts.
//
// Prerequisites (both are checked, with instructions on failure):
//   pnpm install                                       # node_modules present
//   cargo install cargo-about --version 0.8.2 --locked
//
// Usage:
//   node tools/generate-third-party-notices.mjs           # write the file
//   node tools/generate-third-party-notices.mjs --check   # fail if stale
//
// The generated file is meant to be COMMITTED (same policy as
// src/lib/ipc/bindings.ts): the release workflow and the Tauri bundler then
// consume it without needing cargo-about installed. Re-run this script
// whenever dependencies change.
//
// ─── WIRING PENDING THE FIRST GENERATED FILE ────────────────────────────────
// The two consumers below are deliberately NOT in the repo yet: each one hard
// fails while THIRD-PARTY-NOTICES.txt is absent (`tauri build` aborts with
// ResourcePathNotFound — tauri-utils resources.rs — which would red the CI
// bundle smokes; the release job would abort mid-release). Add BOTH, plus the
// README link, in the SAME commit as the first generated file:
//
//   1. src-tauri/tauri.conf.json, inside the "bundle" object:
//        "resources": { "../THIRD-PARTY-NOTICES.txt": "THIRD-PARTY-NOTICES.txt" }
//      The map form puts the repo-root file at the resource root, without
//      Tauri's `_up_` path mangling for paths outside src-tauri.
//
//   2. .github/workflows/release.yml, right after the "Generate SBOM" step:
//        - name: Stage third-party notices
//          shell: bash
//          run: |
//            set -e
//            test -s THIRD-PARTY-NOTICES.txt || {
//              echo "THIRD-PARTY-NOTICES.txt missing or empty" >&2
//              exit 1
//            }
//            cp THIRD-PARTY-NOTICES.txt "dist/lucerna-${APP_VERSION}-THIRD-PARTY-NOTICES.txt"
//      The SHA256SUMS and cosign steps that follow iterate dist/*, so they
//      pick the asset up automatically.
//
//   3. README.md, License section: link THIRD-PARTY-NOTICES.txt.
// ────────────────────────────────────────────────────────────────────────────

import { execSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const OUT = resolve('THIRD-PARTY-NOTICES.txt');
const SEPARATOR = `\n${'-'.repeat(80)}\n`;

/** Names that count as license/notice files inside an npm package. */
const LICENSE_FILE_RE = /^(licen[cs]e|copying|notice)(\.|$)/i;

/**
 * A third-party npm package as rendered into the notices file.
 * @typedef {object} NoticePackage
 * @property {string} name
 * @property {string[]} versions
 * @property {string[]} paths
 * @property {string} license
 * @property {string} homepage
 * @property {string} author
 */

/**
 * One license/notice file found inside a package directory.
 * @typedef {object} LicenseText
 * @property {string} file
 * @property {string} text
 */

/**
 * @param {string} cmd
 * @param {any} [opts] Extra child_process.execSync options (cwd, ...).
 * @returns {string}
 */
function run(cmd, opts = {}) {
  return execSync(cmd, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, ...opts }).toString();
}

/**
 * pnpm has shipped both singular and plural field shapes; accept either.
 * @param {any} plural
 * @param {any} singular
 * @returns {string[]}
 */
function asList(plural, singular) {
  if (Array.isArray(plural)) return plural;
  if (singular) return [singular];
  return [];
}

/**
 * pnpm reports `author` either as a string or as an object with a `name`.
 * @param {any} author
 * @returns {string}
 */
function authorName(author) {
  if (!author) return '';
  if (typeof author === 'string') return author;
  return author.name ?? '';
}

/**
 * Parse `pnpm licenses list --prod --json` output into a sorted flat list.
 * @param {string} json
 * @returns {NoticePackage[]}
 */
export function parsePnpmLicenses(json) {
  const byLicense = JSON.parse(json);
  /** @type {NoticePackage[]} */
  const packages = [];
  for (const [license, entries] of Object.entries(byLicense)) {
    for (const entry of entries) {
      packages.push({
        name: entry.name,
        versions: asList(entry.versions, entry.version),
        paths: asList(entry.paths, entry.path),
        license: entry.license ?? license,
        homepage: entry.homepage ?? '',
        author: authorName(entry.author),
      });
    }
  }
  packages.sort((a, b) => a.name.localeCompare(b.name));
  return packages;
}

/**
 * Read the license/notice file texts shipped inside a package directory.
 * @param {string} dir
 * @returns {LicenseText[]}
 */
export function readLicenseTexts(dir) {
  /** @type {string[]} */
  let names = [];
  try {
    names = readdirSync(dir);
  } catch {
    // Path from pnpm no longer exists — the caller renders the manifest-only
    // fallback line, which says exactly that.
    return [];
  }
  const found = names.filter((n) => LICENSE_FILE_RE.test(n)).sort();
  /** @type {LicenseText[]} */
  const texts = [];
  for (const file of found) {
    try {
      texts.push({ file, text: readFileSync(join(dir, file), 'utf8').trim() });
    } catch {
      texts.push({ file, text: '(unreadable license file)' });
    }
  }
  return texts;
}

/**
 * Compose the npm half of the notices file.
 * @param {NoticePackage[]} packages
 * @param {(dir: string) => LicenseText[]} [readTexts]
 * @returns {string}
 */
export function composeNpmSection(packages, readTexts = readLicenseTexts) {
  /** @type {string[]} */
  const parts = [];
  for (const pkg of packages) {
    /** @type {string[]} */
    const headerLines = [];
    headerLines.push(`${pkg.name} ${pkg.versions.join(', ')}`);
    headerLines.push(`License: ${pkg.license}`);
    if (pkg.homepage) headerLines.push(`Homepage: ${pkg.homepage}`);
    if (pkg.author) headerLines.push(`Author: ${pkg.author}`);

    let body = `(package ships no license file; licensed ${pkg.license} per its manifest)`;
    const texts = pkg.paths.flatMap((p) => readTexts(p));
    if (texts.length > 0) body = texts.map((t) => t.text).join('\n\n');

    parts.push(`${headerLines.join('\n')}\n\n${body}`);
  }
  return parts.join(SEPARATOR);
}

/**
 * @param {string} npmSection
 * @param {string} rustSection
 * @returns {string}
 */
export function composeNotices(npmSection, rustSection) {
  return [
    'THIRD-PARTY NOTICES for Lucerna',
    '',
    'Lucerna itself is GPL-3.0-or-later (see LICENSE). This file lists the',
    'third-party components distributed inside Lucerna release binaries and',
    'their licenses. Regenerate with: node tools/generate-third-party-notices.mjs',
    '',
    '================================================================================',
    'Frontend (npm) components',
    '================================================================================',
    '',
    npmSection,
    '',
    '================================================================================',
    'Backend (Rust crate) components',
    '================================================================================',
    '',
    rustSection,
    '',
  ].join('\n');
}

/**
 * Collect both halves and return the full notices file content.
 * @returns {string}
 */
function generate() {
  if (!existsSync(resolve('node_modules'))) {
    console.error('[notices] node_modules missing — run `pnpm install` first.');
    process.exit(1);
  }

  console.log('[notices] collecting npm production licenses via pnpm...');
  const packages = parsePnpmLicenses(run('pnpm licenses list --prod --json'));
  console.log(`[notices] ${packages.length} npm packages.`);
  const npmSection = composeNpmSection(packages);

  console.log('[notices] generating Rust crate licenses via cargo-about...');
  const srcTauri = resolve('src-tauri');
  try {
    run('cargo about --version', { cwd: srcTauri });
  } catch {
    console.error('[notices] cargo-about is not installed. Install it with:');
    console.error('  cargo install cargo-about --version 0.8.2 --locked');
    process.exit(1);
  }
  // cargo-about picks up src-tauri/about.toml next to the manifest and prints
  // the rendered template to stdout when no `-o` is given (diagnostics go to
  // stderr, which execSync does not capture).
  const rustSection = run('cargo about generate about.hbs', { cwd: srcTauri }).trim();
  return composeNotices(npmSection, rustSection);
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const content = generate();
  if (process.argv.includes('--check')) {
    const current = existsSync(OUT) ? readFileSync(OUT, 'utf8') : '';
    if (current !== content) {
      console.error('[notices] THIRD-PARTY-NOTICES.txt is stale. Run `pnpm notices`.');
      process.exit(1);
    }
    console.log('[notices] THIRD-PARTY-NOTICES.txt is up to date.');
  } else {
    writeFileSync(OUT, content, 'utf8');
    console.log(`[notices] wrote ${OUT}`);
  }
}
