import { describe, expect, it } from 'vitest';
import { formatError } from '$lib/ipc/format-error';

describe('formatError', () => {
  it('formats network errors with URL and details', () => {
    const msg = formatError({
      kind: 'network',
      url: 'https://piston-meta.mojang.com/v1/version.json',
      details: 'connection refused',
    });
    expect(msg).toContain('https://piston-meta.mojang.com/v1/version.json');
    expect(msg).toContain('connection refused');
  });

  it('formats loader_unavailable with brand-canonical loader name', () => {
    const msg = formatError({
      kind: 'loader_unavailable',
      loader: 'quilt',
      mc_version: '1.21',
    });
    expect(msg).toBe('Quilt does not support Minecraft 1.21');
  });

  it('formats neoforge with PascalCase', () => {
    const msg = formatError({
      kind: 'loader_unavailable',
      loader: 'neoforge',
      mc_version: '1.20.1',
    });
    expect(msg).toBe('NeoForge does not support Minecraft 1.20.1');
  });

  it('formats instance_name_too_long with actual and max', () => {
    const msg = formatError({
      kind: 'instance_name_too_long',
      max: 32,
      actual: 50,
    });
    expect(msg).toBe('Instance name is too long: 50/32 characters');
  });

  it('formats hash_mismatch with the offending path', () => {
    const msg = formatError({
      kind: 'hash_mismatch',
      path: 'libraries/foo/bar/1.0/bar-1.0.jar',
      expected: 'abc123',
      got: 'def456',
    });
    expect(msg).toContain('libraries/foo/bar/1.0/bar-1.0.jar');
  });

  it('formats unit variants without field interpolation', () => {
    expect(formatError({ kind: 'already_running' })).toBe('Minecraft is already running');
    expect(formatError({ kind: 'account_not_set' })).toBe(
      'Account not set — enter your name first',
    );
    expect(formatError({ kind: 'last_instance' })).toBe(
      'Cannot delete the last instance — at least one must remain',
    );
    expect(formatError({ kind: 'no_version_selected' })).toBe('Pick a Minecraft version first');
    expect(formatError({ kind: 'instance_name_empty' })).toBe('Instance name cannot be empty');
  });

  it('formats mods_network with url and details', () => {
    const msg = formatError({
      kind: 'mods_network',
      url: 'https://api.modrinth.com/v2/search',
      details: 'timeout',
    });
    expect(msg).toContain('https://api.modrinth.com/v2/search');
    expect(msg).toContain('timeout');
  });

  it('formats mods_platform_auth missing as a key prompt', () => {
    expect(formatError({ kind: 'mods_platform_auth', kind_detail: 'missing' })).toContain(
      'CurseForge requires an API key',
    );
  });

  it('formats mods_sha1_mismatch as a verification failure', () => {
    const msg = formatError({ kind: 'mods_sha1_mismatch', expected: 'aaa', got: 'bbb' });
    expect(msg).toContain('Verification failed');
  });

  it('formats mods_distribution_disabled with the source label', () => {
    const msg = formatError({
      kind: 'mods_distribution_disabled',
      source: 'curseforge',
      project_id: '12345',
    });
    expect(msg.toLowerCase()).toContain('disabled');
  });

  it('formats mods_filename_conflict with the filename', () => {
    const msg = formatError({
      kind: 'mods_filename_conflict',
      filename: 'jei.jar',
      existing_sha: '111',
      incoming_sha: '222',
    });
    expect(msg).toContain('jei.jar');
  });

  it('formats every Modpack* variant', () => {
    expect(formatError({ kind: 'modpack_format_unknown' } as never)).toBe(
      'This file is not a recognised modpack (.mrpack or CurseForge .zip).',
    );
    expect(formatError({ kind: 'modpack_invalid_archive', details: 'bad zip' } as never)).toContain(
      'bad zip',
    );
    expect(
      formatError({
        kind: 'modpack_partial_failure',
        instance_id: 'a',
        failed: [['m', 'r']],
      } as never),
    ).toContain('1 mod(s) skipped');
    expect(formatError({ kind: 'modpack_no_files_selected' } as never)).toBe(
      'Select at least one mod before importing.',
    );
    expect(
      formatError({
        kind: 'modpack_mod_distribution_disabled',
        mod_name: 'FooMod',
        project_url: 'https://...',
      } as never),
    ).toContain('FooMod');
  });
});
