// Appendix A must describe the opener spawn surface as it exists in the
// code: a generalized folder-open row (not the stale single-command row)
// and a default-browser row covering open_url plus the frontend openUrl
// surface. The doc is a shipped transparency claim, so it is a test input.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const principles = readFileSync(resolve(process.cwd(), 'docs/PRINCIPLES.md'), 'utf8');

describe('PRINCIPLES.md Appendix A — opener rows', () => {
  it('replaces the stale open_mods_folder-only row with the generalized folder-open row', () => {
    expect(principles).not.toContain('(currently `<instance>/.minecraft/mods/`)');
    for (const cmd of [
      'open_instance_folder',
      'open_mods_folder',
      'open_saves_folder',
      'open_screenshots_folder',
      'open_backups_folder',
      'open_log_folder',
      'open_launcher_log_folder',
      'open_imported_source_folder',
      'server_open_folder',
      'server_open_logs_folder',
      'server_open_plugins_folder',
      'server_open_mods_folder',
    ]) {
      expect(principles).toContain(cmd);
    }
  });

  it('documents the default-browser spawn and its frontend gate', () => {
    expect(principles).toContain('OS-default web browser');
    expect(principles).toContain('tools/check-opener-calls.mjs');
  });
});
