// The SOLE place the project's CHANGELOG.md is pulled into the bundle.
// Vite's `?raw` suffix inlines the file's text at build time — no runtime
// filesystem read, no network. CHANGELOG.md lives at the repo root (outside
// src/) but within Vite's project root, so the import resolves in dev
// (server.fs allows the workspace root), in `vite build` (Rollup reads the
// file directly), and under Vitest (Vite transform). The path is three levels
// up: changelog/ → lib/ → src/ → repo root.
import raw from '../../../CHANGELOG.md?raw';
import { parseChangelog } from './parse';
import type { Changelog } from './types';

// Parsed once at module load; the changelog is static for a given build.
export const CHANGELOG: Changelog = parseChangelog(raw);
