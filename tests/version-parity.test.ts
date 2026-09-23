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

describe('the three version manifests agree', () => {
  it('package.json, tauri.conf.json and Cargo.toml carry one version', () => {
    const pkg = (JSON.parse(read('package.json')) as { version: string }).version;
    const tauri = (JSON.parse(read('src-tauri', 'tauri.conf.json')) as { version: string }).version;
    // `^version = ` at line start is the [package] table's own line (line 3);
    // dependency versions are indented or inline (`foo = { version = … }`).
    const cargo = /^version = "([^"]+)"$/m.exec(read('src-tauri', 'Cargo.toml'))?.[1];
    expect(pkg).toMatch(/^\d+\.\d+\.\d+/);
    expect({ pkg, tauri, cargo }).toEqual({ pkg, tauri: pkg, cargo: pkg });
  });
});
