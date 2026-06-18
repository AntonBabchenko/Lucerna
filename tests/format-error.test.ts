import { beforeAll, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import type { Error as IpcError } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

describe('formatError', () => {
  beforeAll(() => locale.set('en'));

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

  it('formats mods_sha1_mismatch as a plain verification failure', () => {
    const msg = formatError({ kind: 'mods_sha1_mismatch', expected: 'aaa', got: 'bbb' });
    // Wording softened + raw hashes dropped from the user-facing text.
    expect(msg).toContain('checksum');
    expect(msg).not.toContain('aaa');
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

  it('formats mods_unsafe_filename with the rejected filename', () => {
    const msg = formatError({ kind: 'mods_unsafe_filename', filename: '../../evil.jar' });
    expect(msg).toContain('../../evil.jar');
    expect(msg.toLowerCase()).toContain('unsafe');
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

  it('formats auth_cancelled', () => {
    expect(formatError({ kind: 'auth_cancelled' })).toBe('Microsoft sign-in cancelled.');
  });

  it('formats auth_failed with stage and details', () => {
    expect(formatError({ kind: 'auth_failed', stage: 'xsts', details: 'no Xbox account' })).toBe(
      'Microsoft sign-in failed at xsts: no Xbox account',
    );
  });

  it('formats no_minecraft_profile', () => {
    expect(formatError({ kind: 'no_minecraft_profile' })).toBe(
      "You weren't signed in: this Microsoft account doesn't own Minecraft. Buy a copy and sign in again, or use an account that already owns one.",
    );
  });

  it('formats auth_pending_approval', () => {
    expect(formatError({ kind: 'auth_pending_approval' })).toBe(
      "Microsoft hasn't approved Lucerna's app registration yet. This sign-in will work once approved. Use an offline account in the meantime.",
    );
  });

  it('formats a known error in Russian', () => {
    locale.set('ru');
    const msg = formatError({ kind: 'already_running' });
    expect(msg).toBe('Minecraft уже запущен');
    locale.set('en');
  });

  // Exhaustive table: one sample per Error variant. Typing it as a
  // Record keyed by `IpcError['kind']` makes TypeScript fail the build if a
  // new variant lands in bindings.ts without a sample here — complementing
  // the `_exhaustive: never` guard inside formatError itself. The runtime
  // assertions then prove every variant resolves to real translated copy
  // (not the raw i18n key, not the JSON.stringify exhaustiveness fallback).
  describe('every Error variant resolves to real copy', () => {
    beforeAll(() => locale.set('en'));

    const samples: Record<IpcError['kind'], IpcError> = {
      network: { kind: 'network', url: 'https://x/y', details: 'refused' },
      host_not_allowed: { kind: 'host_not_allowed', url: 'https://x/y' },
      update_check_failed: { kind: 'update_check_failed', details: 'd' },
      update_verification_failed: { kind: 'update_verification_failed', details: 'd' },
      update_install_failed: { kind: 'update_install_failed', details: 'd' },
      hash_mismatch: { kind: 'hash_mismatch', path: 'p.jar', expected: 'a', got: 'b' },
      java_spawn: { kind: 'java_spawn', details: 'no java' },
      already_running: { kind: 'already_running' },
      account_not_set: { kind: 'account_not_set' },
      instance_busy: { kind: 'instance_busy' },
      auth_cancelled: { kind: 'auth_cancelled' },
      auth_failed: { kind: 'auth_failed', stage: 'xsts', details: 'd' },
      no_minecraft_profile: { kind: 'no_minecraft_profile' },
      auth_pending_approval: { kind: 'auth_pending_approval' },
      unknown_version: { kind: 'unknown_version', id: '1.21' },
      loader_unavailable: { kind: 'loader_unavailable', loader: 'fabric', mc_version: '1.21' },
      unsupported_platform: { kind: 'unsupported_platform', os: 'plan9', arch: 'sparc' },
      io: { kind: 'io', path: 'p', details: 'd' },
      last_instance: { kind: 'last_instance' },
      no_version_selected: { kind: 'no_version_selected' },
      instance_not_found: { kind: 'instance_not_found', id: 'i1' },
      forge_promotions_unavailable: { kind: 'forge_promotions_unavailable', flavor: 'forge' },
      forge_maven_metadata_parse_failed: {
        kind: 'forge_maven_metadata_parse_failed',
        details: 'd',
      },
      forge_no_build_for: { kind: 'forge_no_build_for', mc: '1.20.1', fv: '47.0.0' },
      forge_installer_corrupted: {
        kind: 'forge_installer_corrupted',
        mc: '1.20.1',
        fv: '47.0.0',
        details: 'd',
      },
      forge_unsupported_processor: { kind: 'forge_unsupported_processor', coord: 'a:b:c' },
      forge_patcher_failed: { kind: 'forge_patcher_failed', processor: 'ss', details: 'd' },
      forge_mappings_missing: { kind: 'forge_mappings_missing', mc: '1.20.1' },
      instance_name_empty: { kind: 'instance_name_empty' },
      instance_name_too_long: { kind: 'instance_name_too_long', max: 32, actual: 50 },
      mods_network: { kind: 'mods_network', url: 'https://x', details: 'd' },
      mods_platform_auth: { kind: 'mods_platform_auth', kind_detail: 'missing' },
      mods_platform_unreachable: {
        kind: 'mods_platform_unreachable',
        url: 'https://api.curseforge.com',
      },
      mods_distribution_disabled: {
        kind: 'mods_distribution_disabled',
        source: 'curseforge',
        project_id: '1',
      },
      mods_not_found: { kind: 'mods_not_found', source: 'modrinth' },
      mods_platform_unsupported: { kind: 'mods_platform_unsupported', source: 'ftb' },
      mods_decode: { kind: 'mods_decode', source: 'modrinth', details: 'd' },
      mods_sha1_unavailable: { kind: 'mods_sha1_unavailable' },
      mods_sha1_mismatch: { kind: 'mods_sha1_mismatch', expected: 'a', got: 'b' },
      mods_dependency_unresolvable: {
        kind: 'mods_dependency_unresolvable',
        project_ref: 'jei',
      },
      mods_filename_conflict: {
        kind: 'mods_filename_conflict',
        filename: 'jei.jar',
        existing_sha: '1',
        incoming_sha: '2',
      },
      mods_unsafe_filename: { kind: 'mods_unsafe_filename', filename: '../../evil.jar' },
      mods_cache_io: { kind: 'mods_cache_io', details: 'd' },
      mods_instance_path: { kind: 'mods_instance_path', path: 'p', details: 'd' },
      modpack_invalid_archive: { kind: 'modpack_invalid_archive', details: 'd' },
      modpack_format_unknown: { kind: 'modpack_format_unknown' },
      modpack_manifest_invalid: {
        kind: 'modpack_manifest_invalid',
        format: 'mrpack',
        details: 'd',
      },
      modpack_unsupported_manifest_version: {
        kind: 'modpack_unsupported_manifest_version',
        format: 'mrpack',
        version: 2,
      },
      modpack_unsupported_loader: {
        kind: 'modpack_unsupported_loader',
        format: 'mrpack',
        loader_id: 'liteloader',
      },
      modpack_download_host_not_allowed: {
        kind: 'modpack_download_host_not_allowed',
        host: 'evil.test',
        file_path: 'mods/x.jar',
      },
      modpack_sha1_unavailable: { kind: 'modpack_sha1_unavailable', mod_name: 'JEI' },
      modpack_mod_distribution_disabled: {
        kind: 'modpack_mod_distribution_disabled',
        mod_name: 'JEI',
        project_url: 'https://x',
      },
      modpack_overrides_path_escape: { kind: 'modpack_overrides_path_escape', entry: '../x' },
      modpack_overrides_too_large: {
        kind: 'modpack_overrides_too_large',
        entry: 'x',
        size: 999,
        cap: 100,
      },
      modpack_no_files_selected: { kind: 'modpack_no_files_selected' },
      modpack_instance_creation_failed: {
        kind: 'modpack_instance_creation_failed',
        details: 'd',
      },
      modpack_partial_failure: {
        kind: 'modpack_partial_failure',
        instance_id: 'i1',
        failed: [['m', 'r']],
      },
      modpack_bundled_no_url: { kind: 'modpack_bundled_no_url', mod_name: 'JEI' },
      modpack_cf_distribution_disabled: {
        kind: 'modpack_cf_distribution_disabled',
        pack_name: 'ATM9',
      },
      modpack_export_failed: { kind: 'modpack_export_failed', details: 'd' },
      world_not_found: { kind: 'world_not_found', instance_id: 'i1', folder_name: 'world' },
      world_in_use: { kind: 'world_in_use', folder_name: 'world' },
      world_path_invalid: { kind: 'world_path_invalid', name: 'world', reason: 'bad' },
      world_name_unresolvable: { kind: 'world_name_unresolvable', folder_name: 'world' },
      backup_not_found: {
        kind: 'backup_not_found',
        instance_id: 'i1',
        world_folder: 'world',
        filename: 'b.zip',
      },
      backup_corrupt: { kind: 'backup_corrupt', filename: 'b.zip', details: 'd' },
      world_import_not_a_world: { kind: 'world_import_not_a_world' },
      world_import_unsupported_source: { kind: 'world_import_unsupported_source' },
      world_import_invalid_archive: { kind: 'world_import_invalid_archive', details: 'd' },
      world_import_too_large: { kind: 'world_import_too_large', size: 3, cap: 2 },
      playtime_io: { kind: 'playtime_io', details: 'd' },
      tray_io: { kind: 'tray_io', details: 'd' },
      window_io: { kind: 'window_io', details: 'd' },
      mc_logs_upload: { kind: 'mc_logs_upload', details: 'd' },
      quick_play_address_invalid: {
        kind: 'quick_play_address_invalid',
        address: 'bad host',
        reason: 'contains whitespace or control characters',
      },
      import_instance_unreadable: {
        kind: 'import_instance_unreadable',
        launcher: 'prism',
        details: 'mmc-pack.json missing',
      },
      import_unsupported_loader: { kind: 'import_unsupported_loader', loader: 'liteloader' },
      import_source_unrecognized: { kind: 'import_source_unrecognized', path: 'C:/tmp/x' },
      import_no_provenance: { kind: 'import_no_provenance', id: 'abc' },
      import_source_missing: { kind: 'import_source_missing', path: 'C:/x/y' },
      servers_dat_parse: { kind: 'servers_dat_parse', reason: 'bad tag' },
      saved_server_name_invalid: {
        kind: 'saved_server_name_invalid',
        name: 'x',
        reason: 'empty name',
      },
      saved_server_list_changed: { kind: 'saved_server_list_changed' },
      server_invalid_property: {
        kind: 'server_invalid_property',
        key: 'max-players',
        value: '-1',
        reason: 'must be positive',
      },
      server_eula_not_accepted: { kind: 'server_eula_not_accepted' },
      server_jar_unavailable: {
        kind: 'server_jar_unavailable',
        loader: 'fabric',
        mc_version: '1.21',
        reason: 'no server download in manifest',
      },
      server_installer_failed: {
        kind: 'server_installer_failed',
        loader: 'forge',
        details: 'exit 1',
      },
      server_spawn_failed: { kind: 'server_spawn_failed', details: 'ENOENT java' },
      server_already_running: { kind: 'server_already_running', id: 'srv-1' },
      server_not_running: { kind: 'server_not_running', id: 'srv-1' },
      upload_not_configured: { kind: 'upload_not_configured' },
      sftp_connect_failed: { kind: 'sftp_connect_failed', details: 'connection refused' },
      sftp_auth_failed: { kind: 'sftp_auth_failed', details: 'wrong password' },
      sftp_host_key_mismatch: { kind: 'sftp_host_key_mismatch', expected: 'aabbcc', got: 'ddeeff' },
      sftp_transfer_failed: { kind: 'sftp_transfer_failed', details: 'disk full' },
    };

    it.each(Object.entries(samples))('renders real copy for %s', (_kind, sample) => {
      const msg = formatError(sample);
      // Non-empty string.
      expect(typeof msg).toBe('string');
      expect(msg.length).toBeGreaterThan(0);
      // Not the JSON.stringify exhaustiveness fallback (variant has a case).
      expect(msg).not.toBe(JSON.stringify(sample));
      // i18n resolved — a missing key echoes the raw `errors.<key>` path, and
      // every key passed in formatError starts with `errors.`.
      expect(msg.startsWith('errors.')).toBe(false);
    });

    it('has a distinct sample for every Error variant (Record completeness)', () => {
      // The Record type fails the build if a `kind` is missing. This exact
      // count is the runtime complement: a duplicate key in the literal would
      // collapse two entries into one and drop the length below the total,
      // which the type system does NOT catch. Bump this when variants change.
      expect(Object.keys(samples)).toHaveLength(95);
    });
  });

  describe('io error truncation', () => {
    it('passes details under 120 code points through unchanged', () => {
      const msg = formatError({
        kind: 'io',
        path: 'C:/x/account.json',
        details: 'short message',
      });
      expect(msg).toBe('IO error at C:/x/account.json: short message');
    });

    it('truncates long ASCII details at 120 code points + ellipsis hint', () => {
      const longDetails = 'a'.repeat(200);
      const msg = formatError({
        kind: 'io',
        path: 'C:/x/account.json',
        details: longDetails,
      });
      expect(msg).toContain('a'.repeat(120));
      expect(msg).toContain('… (open Logs for full text)');
      expect(msg).not.toContain('a'.repeat(121));
    });

    it('does not split a surrogate pair at the 120 boundary', () => {
      // Position 119 (the 120th code point) is an emoji — its UTF-16
      // representation is a surrogate pair. UTF-16-based slice would
      // split it and leave a lone surrogate (and then the … hint), so
      // the rendered output would contain a U+FFFD replacement glyph.
      const before = 'a'.repeat(119);
      const after = 'b'.repeat(20);
      const details = `${before}🎉${after}`;
      const msg = formatError({
        kind: 'io',
        path: 'C:/x/file',
        details,
      });
      // Code-point slicing keeps the emoji intact at position 119.
      expect(msg).toContain('🎉');
      // No lone surrogate in the output — iterate by code point. A
      // code-point iteration that yields a value in [D800,DFFF] means a
      // lone surrogate slipped through. Anything outside that band
      // (including a full astral code point like U+1F389) is fine.
      for (const ch of msg) {
        const cp = ch.codePointAt(0) ?? 0;
        const isLoneSurrogate = cp >= 0xd800 && cp <= 0xdfff;
        expect(isLoneSurrogate).toBe(false);
      }
      expect(msg).not.toContain('�');
      expect(msg).toContain('… (open Logs for full text)');
    });
  });
});
