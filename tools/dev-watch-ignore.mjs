// Root-anchored ignore rule for the dev server's file watcher (`server.watch.ignored` in
// vite.config.js). Lives here, not inline in the config, so tests/dev-watch-ignore.test.ts
// can pin it without starting Vite.
//
// WHY A FUNCTION AND NOT A GLOB
// Vite hands its ABSOLUTE project root to chokidar, so every path the watcher tests is
// absolute, and a string entry such as `**/.claude/**` is matched against that absolute
// path (chokidar 3.6 as bundled by vite 6: anymatch + picomatch, no `cwd`). Agent
// worktrees are full checkouts living at `<main>/.claude/worktrees/<name>/` and
// `<main>/.claude-worktrees/<name>/` — a dev server started from one of them watches a
// root whose own path contains `/.claude/`. The glob then matches the root itself, the
// crawl is pruned there, and hot reload is silently off: no `hmr update` lines, and Vite
// keeps serving its cached transform of every module until it is restarted.
//
// So these directories are ignored only as DIRECT children of the project root, decided
// on the path RELATIVE to that root. A function also beats an absolute glob string
// (`<root>/.claude/**`) on three counts: `path.win32.relative` is case-insensitive, so a
// `c:` vs `C:` drive letter cannot quietly disable the rule (picomatch is case-sensitive);
// the root needs no glob-escaping; and chokidar 4 dropped globs but kept functions.

import nodePath from 'node:path';

/**
 * Directory names that are agent worktree containers or cargo target dirs:
 * `.claude`, `.claude-worktrees`, and anything starting with `.cargo-target`
 * (per-worktree targets are named `.cargo-target-<name>`).
 *
 * @param {string} name a single path segment
 * @returns {boolean}
 */
function isAgentOrCargoDir(name) {
  return name === '.claude' || name === '.claude-worktrees' || name.startsWith('.cargo-target');
}

/**
 * Builds the watcher matcher that ignores the agent-worktree and cargo-target directories
 * sitting directly under `root` — the directory itself (chokidar tests a directory by its
 * own path before descending, so this is what prunes the crawl) and everything below it.
 *
 * Everything else is left to the other `ignored` entries: the root itself, paths nested
 * deeper (`<root>/src/.claude/…`), and paths outside the root (Vite watches a few of those
 * explicitly — config dependencies, env files — and they must stay watched).
 *
 * FAILURE DIRECTION: a path that cannot be related to the root is NOT ignored. That covers
 * another volume, and also the same directory spelled in a different Windows form — an
 * extended-length `\\?\C:\…` on one side only makes `path.relative` hand back the absolute
 * path. Both values come from the same `process.cwd()`, so this should not happen; if it
 * ever does, the watcher over-watches, which fails LOUDLY ("changed tsconfig file detected"
 * in the log, then the `respond is not a function` 500s described in vite.config.js).
 * Guessing "ignored" instead would fail silently — hot reload quietly off, which is the
 * very bug this module replaced.
 *
 * @param {string} root absolute project root: the directory holding vite.config.js
 * @param {import('node:path').PlatformPath} [path] path flavour; injectable so the
 *   Windows behaviour is testable on a POSIX runner and vice versa
 * @returns {(watchedPath: string) => boolean} anymatch-compatible matcher; chokidar 3
 *   passes the path with forward slashes, native separators work just as well
 */
export function createRootDirIgnore(root, path = nodePath) {
  return (watchedPath) => {
    // '' for the root itself, '..' for anything outside it, and on Windows a drive such
    // as 'D:' for another volume — none of which is an agent or cargo directory name, so
    // the one test below also covers "not inside the root at all".
    const [topSegment] = path.relative(root, watchedPath).split(path.sep, 1);
    return isAgentOrCargoDir(topSegment);
  };
}
