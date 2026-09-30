import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const body = readFileSync(
  join(import.meta.dirname, '../src/lib/mods/installed/InstalledModsView.svelte'),
  'utf8',
);
// From the detail-install handler up to the function after the one it delegates
// to: `installDetailVersion` and `runVersionInstall`, then `setEnabled`.
const start = body.indexOf('async function installDetailVersion(');
const end = body.indexOf('async function setEnabled(', start);
const detailInstall = body.slice(start, end);

describe('Installed tab — installing from the detail modal', () => {
  it('finds the handler it is about to judge', () => {
    // Guards the guard: a renamed function would otherwise make every
    // `not.toContain` below pass on an empty string — and a lost end anchor
    // (`slice(start, -1)`) would stretch the scan over the rest of the file.
    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThan(start);
    expect(detailInstall).toContain('async function runVersionInstall(');
  });

  it('never uninstalls before installing', () => {
    expect(detailInstall).not.toContain('modsUninstall');
  });

  it('switches through the update command, decided by the shared rule', () => {
    expect(detailInstall).toContain('switchTarget(');
    expect(detailInstall).toContain('updateMod(');
  });

  it('asks instead of toasting when the backend refuses', () => {
    expect(detailInstall).toContain('askOffPlatform(');
    expect(body).toContain('offPlatformFactsOfError(');
    expect(body).toContain('<CompatWarningDialog');
  });

  it('tells the detail modal the bytes of the installed build, not only its id', () => {
    expect(body).toContain('installedSha1={detailInstalled?.sha1 ?? null}');
  });
});
