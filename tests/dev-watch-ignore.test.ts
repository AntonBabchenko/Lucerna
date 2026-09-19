/**
 * The dev server must keep watching its OWN sources when it is started from an agent
 * worktree, i.e. from a checkout that lives at `<main>/.claude/worktrees/<name>/`.
 *
 * `vite.config.js` keeps the watcher out of `.claude/`, `.claude-worktrees/` and
 * `.cargo-target*` because a MAIN-checkout dev server that crawls the worktrees nested in
 * its root gets `moduleGraph.invalidateAll()` mid-request (vitejs/vite#22293). Those rules
 * used to be free-floating globs: a `.claude` path segment at ANY depth. The watcher tests
 * ABSOLUTE paths, and inside a worktree every absolute path contains `/.claude/` — so the
 * glob matched the root itself, the crawl was pruned there, and hot reload was silently
 * off for all feature work (2026-09-20: an edited `.svelte` file kept being served from
 * Vite's transform cache until the dev server was restarted).
 *
 * The rules are now one function anchored to the project root. It is a plain function so
 * this file can pin its behaviour without starting Vite; the path flavour is injected so
 * the Windows cases (drive-letter case, backslashes) also run on the Linux CI runner.
 */
import path from 'node:path';
import { describe, expect, test } from 'vitest';
import { createRootDirIgnore } from '../tools/dev-watch-ignore.mjs';
import viteConfig from '../vite.config.js';

describe('createRootDirIgnore — Windows paths', () => {
  const MAIN = 'C:\\Projects\\Lucerna';
  const WORKTREE = 'C:\\Projects\\Lucerna\\.claude\\worktrees\\some-feature';
  const LEGACY_WORKTREE = 'C:\\Projects\\Lucerna\\.claude-worktrees\\some-feature';

  describe('dev server started from the MAIN checkout', () => {
    const isIgnored = createRootDirIgnore(MAIN, path.win32);

    test.each([
      // The directory itself: chokidar tests a directory by its own path before it
      // descends, so this is the case that actually prunes the crawl.
      ['C:/Projects/Lucerna/.claude'],
      ['C:/Projects/Lucerna/.claude/worktrees/x/src/lib/Foo.svelte'],
      // The write that triggers the upstream bug: a nested worktree's `svelte-kit sync`.
      ['C:/Projects/Lucerna/.claude/worktrees/x/.svelte-kit/tsconfig.json'],
      ['C:/Projects/Lucerna/.claude-worktrees/x/src/routes/+page.svelte'],
      ['C:/Projects/Lucerna/.cargo-target'],
      ['C:/Projects/Lucerna/.cargo-target-some-feature/debug/deps/lucerna.rlib'],
      // chokidar 3 normalises to forward slashes before calling a function matcher;
      // native separators must behave the same should that ever change.
      ['C:\\Projects\\Lucerna\\.claude\\worktrees\\x\\src\\a.ts'],
      // A shell started with a lowercase drive letter reports `c:\…` for the same root.
      ['c:/projects/lucerna/.claude/worktrees/x/src/a.ts'],
    ])('ignores %s', (watchedPath) => {
      expect(isIgnored(watchedPath)).toBe(true);
    });

    test.each([
      ['C:/Projects/Lucerna'],
      ['C:/Projects/Lucerna/src/lib/Foo.svelte'],
      ['C:/Projects/Lucerna/vite.config.js'],
      // Only DIRECT children of the root are agent/cargo directories.
      ['C:/Projects/Lucerna/src/.claude/notes.ts'],
      // Lookalike names.
      ['C:/Projects/Lucerna/.claudette/a.ts'],
      ['C:/Projects/Lucerna/.claude-worktrees-old/a.ts'],
      ['C:/Projects/Lucerna/cargo-target/a.ts'],
      // Outside the root is not this rule's business (Vite watches a few such files
      // explicitly: config dependencies, env files).
      ['C:/Projects/.claude/a.ts'],
      ['D:/elsewhere/.claude/a.ts'],
    ])('watches %s', (watchedPath) => {
      expect(isIgnored(watchedPath)).toBe(false);
    });
  });

  describe('root spellings', () => {
    test.each([
      ['a trailing separator', 'C:\\Projects\\Lucerna\\', 'C:/Projects/Lucerna/.claude/a.ts'],
      ['a UNC share', '\\\\srv\\share\\Lucerna', '//srv/share/Lucerna/.claude/a.ts'],
      [
        'an extended-length prefix on both sides',
        '\\\\?\\C:\\Projects\\Lucerna',
        '\\\\?\\C:\\Projects\\Lucerna\\.claude\\a.ts',
      ],
    ])('anchors to a root written with %s', (_label, root, watchedPath) => {
      expect(createRootDirIgnore(root, path.win32)(watchedPath)).toBe(true);
    });

    // FAILURE DIRECTION, pinned on purpose. With the extended-length prefix on ONE side
    // only, `path.relative` cannot relate the two and the matcher answers "not ignored".
    // Over-watching fails loudly (tsconfig reload lines, then `respond is not a function`);
    // answering "ignored" would fail silently, as the old globs did. Whoever teaches the
    // matcher to strip the prefix flips these two expectations deliberately.
    test.each([
      ['C:\\Projects\\Lucerna', '\\\\?\\C:\\Projects\\Lucerna\\.claude\\a.ts'],
      ['\\\\?\\C:\\Projects\\Lucerna', 'C:/Projects/Lucerna/.claude/a.ts'],
    ])('a path it cannot relate to root %s is watched, not silently dropped', (root, watched) => {
      expect(createRootDirIgnore(root, path.win32)(watched)).toBe(false);
    });
  });

  describe.each([
    ['.claude/worktrees', WORKTREE],
    ['.claude-worktrees', LEGACY_WORKTREE],
  ])('dev server started from a checkout under %s', (_label, root) => {
    const isIgnored = createRootDirIgnore(root, path.win32);
    const inRoot = (relative: string) => `${root.replaceAll('\\', '/')}/${relative}`;

    test('watches the root itself — ignoring it prunes the whole crawl', () => {
      expect(isIgnored(root)).toBe(false);
      expect(isIgnored(root.replaceAll('\\', '/'))).toBe(false);
    });

    test.each([
      ['src/lib/instances/ManageInstanceList.svelte'],
      ['src/routes/+page.svelte'],
      ['src/app.css'],
      ['vite.config.js'],
      ['svelte.config.js'],
      ['.svelte-kit/generated/root.svelte'],
      ['static/favicon.png'],
    ])('watches its own %s', (relative) => {
      expect(isIgnored(inRoot(relative))).toBe(false);
    });

    test.each([
      ['.claude/settings.local.json'],
      ['.claude/worktrees/nested/src/a.ts'],
      ['.claude-worktrees/nested/src/a.ts'],
      ['.cargo-target/debug/deps/lucerna.rlib'],
    ])('still ignores its own %s', (relative) => {
      expect(isIgnored(inRoot(relative))).toBe(true);
    });
  });
});

describe('createRootDirIgnore — POSIX paths', () => {
  const MAIN = '/home/dev/Lucerna';
  const WORKTREE = '/home/dev/Lucerna/.claude/worktrees/some-feature';

  test('main checkout: ignores the nested agent and cargo directories, watches src', () => {
    const isIgnored = createRootDirIgnore(MAIN, path.posix);

    expect(isIgnored(`${MAIN}/.claude`)).toBe(true);
    expect(isIgnored(`${MAIN}/.claude/worktrees/x/.svelte-kit/tsconfig.json`)).toBe(true);
    expect(isIgnored(`${MAIN}/.claude-worktrees/x/src/a.ts`)).toBe(true);
    expect(isIgnored(`${MAIN}/.cargo-target-x/debug/a.rlib`)).toBe(true);
    expect(isIgnored(MAIN)).toBe(false);
    expect(isIgnored(`${MAIN}/src/lib/Foo.svelte`)).toBe(false);
    expect(isIgnored('/home/dev/.claude/a.ts')).toBe(false);
  });

  test('worktree checkout: watches its own sources, ignores its own nested directories', () => {
    const isIgnored = createRootDirIgnore(WORKTREE, path.posix);

    expect(isIgnored(WORKTREE)).toBe(false);
    expect(isIgnored(`${WORKTREE}/src/lib/Foo.svelte`)).toBe(false);
    expect(isIgnored(`${WORKTREE}/vite.config.js`)).toBe(false);
    expect(isIgnored(`${WORKTREE}/.claude/settings.local.json`)).toBe(true);
    expect(isIgnored(`${WORKTREE}/.cargo-target/debug/a.rlib`)).toBe(true);
  });
});

describe('vite.config.js wiring', () => {
  // Not `new URL('..', import.meta.url)`: Vite rewrites that pattern into an asset URL
  // (`http://localhost:3000/@fs/…`) in every module Vitest transforms.
  const repoRoot = path.resolve(import.meta.dirname, '..');

  async function resolveIgnored() {
    const config = await viteConfig({ command: 'serve', mode: 'development' });
    // `plugins` holds the un-awaited `sveltekit()` promise. Settle it here so a rejection
    // fails THIS test instead of surfacing later as an unhandled one.
    await Promise.all(config.plugins ?? []);
    return [config.server?.watch?.ignored ?? []].flat();
  }

  test('no free-floating glob names an agent directory', async () => {
    const globs = (await resolveIgnored()).filter((entry) => typeof entry === 'string');

    // A string entry is matched against the absolute path, so `**/.claude/**` also matches
    // every file of a checkout that lives under `.claude/worktrees/`. Agent directories
    // belong to the root-anchored function, never to a glob.
    expect(globs.filter((glob) => glob.includes('.claude'))).toEqual([]);
    // The two rules that are safe as globs, and must keep working in a worktree too.
    expect(globs).toContain('**/src-tauri/**');
    expect(globs).toContain('**/.svelte-kit/tsconfig.json');
  });

  test('the function entries are anchored to the directory of vite.config.js', async () => {
    const matchers = (await resolveIgnored()).filter((entry) => typeof entry === 'function');
    const isIgnored = (watchedPath: string) => matchers.some((matches) => matches(watchedPath));

    expect(isIgnored(path.join(repoRoot, '.claude', 'worktrees', 'x', 'src', 'a.ts'))).toBe(true);
    expect(isIgnored(path.join(repoRoot, '.claude-worktrees', 'x', 'src', 'a.ts'))).toBe(true);
    expect(isIgnored(path.join(repoRoot, '.cargo-target', 'debug', 'a.rlib'))).toBe(true);
    // When this suite itself runs from a `.claude/worktrees/*` checkout, the next two lines
    // ARE the regression: `repoRoot` then contains `/.claude/`.
    expect(isIgnored(path.join(repoRoot, 'src', 'lib', 'a.ts'))).toBe(false);
    expect(isIgnored(repoRoot)).toBe(false);
  });
});
