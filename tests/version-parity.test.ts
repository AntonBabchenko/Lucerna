// ABOUT-11: About reads package.json; the installer reads tauri.conf.json; the
// binary reports Cargo.toml. A release bump that misses one ships three
// versions. Three files, one string — compared here so the miss is a red CI,
// not a support ticket.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

function read(...segments: string[]): string {
  return readFileSync(join(process.cwd(), ...segments), 'utf8');
}

/**
 * The body of a TOML table, from its header to the next one (or the end).
 * Scoping matters: `[dependencies.foo]` tables carry their own line-start
 * `version = "…"`, so a search over the whole file reads whichever table
 * happens to come first — today that is `[package]`, and nothing guarantees
 * it stays that way.
 */
function tomlTable(source: string, name: string): string {
  const header = new RegExp(`^\\[${name}\\]\\s*$`, 'm').exec(source);
  if (header === null) throw new Error(`no [${name}] table`);
  const rest = source.slice(header.index + header[0].length);
  const next = /^\[/m.exec(rest);
  return next === null ? rest : rest.slice(0, next.index);
}

function packageVersion(cargoToml: string): string | undefined {
  return /^version = "([^"]+)"$/m.exec(tomlTable(cargoToml, 'package'))?.[1];
}

describe('the three version manifests agree', () => {
  it('package.json, tauri.conf.json and Cargo.toml carry one version', () => {
    const pkg = (JSON.parse(read('package.json')) as { version: string }).version;
    const tauri = (JSON.parse(read('src-tauri', 'tauri.conf.json')) as { version: string }).version;
    const cargo = packageVersion(read('src-tauri', 'Cargo.toml'));
    expect(pkg).toMatch(/^\d+\.\d+\.\d+/);
    expect({ pkg, tauri, cargo }).toEqual({ pkg, tauri: pkg, cargo: pkg });
  });

  it('reads the [package] version, not a dependency table that carries one', () => {
    // The guard has to survive the file it watches: the day someone writes a
    // dependency as its own table above [package], an unscoped match silently
    // compares the wrong string and this suite goes green on a real drift.
    const toml = [
      '[dependencies.zzz]',
      'version = "9.9.9"',
      '',
      '[package]',
      'name = "lucerna"',
      'version = "1.2.3"',
      '',
      '[dependencies]',
      'serde = { version = "1.0.0" }',
    ].join('\n');
    expect(packageVersion(toml)).toBe('1.2.3');
  });
});
