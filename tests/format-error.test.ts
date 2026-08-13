import { beforeAll, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import type { Error as IpcError } from '$lib/ipc/bindings';
import { ERROR_CLASS, formatError, withDetailTail } from '$lib/ipc/format-error';

describe('formatError', () => {
  beforeAll(() => locale.set('en'));

  it('formats network as a clean actionable message — no url/detail leak', () => {
    const msg = formatError({
      kind: 'network',
      url: 'https://piston-meta.mojang.com/v1/version.json',
      details: 'connection refused',
    });
    expect(msg).not.toContain('https://piston-meta.mojang.com/v1/version.json');
    expect(msg).not.toContain('connection refused');
    expect(msg.toLowerCase()).toContain('internet');
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
    expect(formatError({ kind: 'already_running', instance_id: 'abc' })).toBe(
      'Minecraft is already running',
    );
    expect(formatError({ kind: 'account_not_set' })).toBe(
      'Account not set — enter your name first',
    );
    expect(formatError({ kind: 'last_instance' })).toBe(
      'Cannot delete the last instance — at least one must remain',
    );
    expect(formatError({ kind: 'no_version_selected' })).toBe('Pick a Minecraft version first');
    expect(formatError({ kind: 'instance_name_empty' })).toBe('Instance name cannot be empty');
  });

  it('formats mods_network as a clean actionable message — no triple URL, no English jargon', () => {
    const msg = formatError({
      kind: 'mods_network',
      url: 'https://api.modrinth.com/v2/search',
      details: 'error sending request for url (https://api.modrinth.com/v2/search)',
    });
    expect(msg).not.toContain('https://api.modrinth.com/v2/search');
    expect(msg).not.toContain('error sending request');
    expect(msg.toLowerCase()).toContain('internet');
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

  it('formats the import-by-link variants with their own reason text', () => {
    // Both are `clean`: the reason is built in Rust from the link itself, so it
    // renders verbatim — no truncation, no "open Logs" tail.
    expect(
      formatError({ kind: 'import_url_invalid', reason: "unsupported scheme 'file'" } as never),
    ).toBe("That link can't be imported: unsupported scheme 'file'");
    expect(
      formatError({ kind: 'import_url_unsupported_source', platform: 'FTB' } as never),
    ).toContain('FTB');
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
    ).toContain('1 mod skipped');
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

  it('renders offline_name_invalid with the name and a localized reason', () => {
    const msg = formatError({
      kind: 'offline_name_invalid',
      name: 'Игрок',
      reason: 'invalid_chars',
    });
    expect(msg).toContain('Игрок');
    expect(msg).not.toMatch(/^\{/);
    expect(msg).toContain('Latin');
  });

  it('formats a known error in Russian', () => {
    locale.set('ru');
    const msg = formatError({ kind: 'already_running', instance_id: 'abc' });
    expect(msg).toBe('Minecraft уже запущен');
    locale.set('en');
  });

  // A channel missing from CONSENTED_CHANNEL_LABELS still renders a whole
  // sentence — it just names nothing. So asserting "the message mentions the
  // setting" is not enough on its own; the last assertion is what proves the
  // channel is actually mapped rather than falling through to the generic.
  it('names the Settings toggle behind each consented channel', () => {
    const ping = formatError({ kind: 'consented_channel_disabled', channel: 'server_ping' });
    expect(ping).toContain('Show status for my saved servers');

    const ai = formatError({ kind: 'consented_channel_disabled', channel: 'ai_translation' });
    expect(ai).toContain('AI translation');

    const unmapped = formatError({ kind: 'consented_channel_disabled', channel: 'not_a_channel' });
    expect(ai).not.toBe(unmapped);
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
      network: { kind: 'network', url: 'https://leak.test/net', details: 'RAW_TRANSPORT_DETAIL' },
      host_not_allowed: { kind: 'host_not_allowed', url: 'https://x/y' },
      consented_channel_disabled: { kind: 'consented_channel_disabled', channel: 'server_ping' },
      update_check_failed: { kind: 'update_check_failed', details: 'd' },
      update_verification_failed: { kind: 'update_verification_failed', details: 'd' },
      update_install_failed: { kind: 'update_install_failed', details: 'd' },
      hash_mismatch: { kind: 'hash_mismatch', path: 'p.jar', expected: 'a', got: 'b' },
      java_spawn: { kind: 'java_spawn', details: 'no java' },
      already_running: { kind: 'already_running', instance_id: 'abc' },
      account_not_set: { kind: 'account_not_set' },
      instance_busy: { kind: 'instance_busy' },
      auth_cancelled: { kind: 'auth_cancelled' },
      auth_failed: { kind: 'auth_failed', stage: 'xsts', details: 'd' },
      no_minecraft_profile: { kind: 'no_minecraft_profile' },
      auth_pending_approval: { kind: 'auth_pending_approval' },
      cosmetic_image_invalid: { kind: 'cosmetic_image_invalid', details: 'skin must be 64x64' },
      skin_library: { kind: 'skin_library', details: 'entry not found' },
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
      instance_dir_name_empty: { kind: 'instance_dir_name_empty' },
      instance_dir_name_taken: { kind: 'instance_dir_name_taken', name: 'My-Pack' },
      instance_dir_name_reserved: { kind: 'instance_dir_name_reserved', name: 'CON' },
      instance_dir_locked: { kind: 'instance_dir_locked', name: 'My-Pack' },
      path_not_launchable: { kind: 'path_not_launchable', data_root: false },
      mods_network: {
        kind: 'mods_network',
        url: 'https://leak.test/mods',
        details: 'RAW_TRANSPORT_DETAIL',
      },
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
      changelog_unsupported: { kind: 'changelog_unsupported' },
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
      import_url_invalid: { kind: 'import_url_invalid', reason: 'unsupported scheme' },
      import_url_unsupported_source: { kind: 'import_url_unsupported_source', platform: 'ftb' },
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
      world_restore_stranded: {
        kind: 'world_restore_stranded',
        world_folder: 'World1',
        recovered_at: '.tmp-restoring-World1-0',
      },
      world_recover_target_occupied: {
        kind: 'world_recover_target_occupied',
        world_folder: 'World1',
      },
      screenshot_not_found: {
        kind: 'screenshot_not_found',
        instance_id: 'i1',
        filename: 'shot.png',
      },
      screenshot_path_invalid: { kind: 'screenshot_path_invalid', name: '../x', reason: 'bad' },
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
      level_dat_parse: { kind: 'level_dat_parse', reason: 'invalid tag id 99' },
      saved_server_name_invalid: {
        kind: 'saved_server_name_invalid',
        name: 'x',
        reason: 'empty name',
      },
      offline_name_invalid: {
        kind: 'offline_name_invalid',
        name: 'Игрок',
        reason: 'invalid_chars',
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
      server_world_not_created: { kind: 'server_world_not_created' },
      server_upload_in_progress: { kind: 'server_upload_in_progress', id: 'srv-1' },
      upload_cancelled: { kind: 'upload_cancelled' },
      server_not_running: { kind: 'server_not_running', id: 'srv-1' },
      server_mod_required_by_other: {
        kind: 'server_mod_required_by_other',
        filename: 'libraryferret.jar',
        required_by: 'bettervillage.jar',
      },
      server_file_invalid: {
        kind: 'server_file_invalid',
        filename: 'C:evil.jar',
        reason: 'invalid filename',
      },
      server_core_unsupported: {
        kind: 'server_core_unsupported',
        reason: 'this server core does not load mods',
      },
      server_content_stale: { kind: 'server_content_stale' },
      server_name_invalid: { kind: 'server_name_invalid', reason: 'duplicate' },
      upload_not_configured: { kind: 'upload_not_configured' },
      sftp_connect_failed: { kind: 'sftp_connect_failed', details: 'connection refused' },
      sftp_auth_failed: { kind: 'sftp_auth_failed', details: 'wrong password' },
      sftp_host_key_mismatch: { kind: 'sftp_host_key_mismatch', expected: 'aabbcc', got: 'ddeeff' },
      sftp_transfer_failed: { kind: 'sftp_transfer_failed', details: 'disk full' },
      server_import_unsupported_source: { kind: 'server_import_unsupported_source' },
      server_import_invalid_archive: { kind: 'server_import_invalid_archive', details: 'bad zip' },
      server_import_too_large: { kind: 'server_import_too_large', size: 3, cap: 2 },
      server_import_not_a_server: { kind: 'server_import_not_a_server' },
      server_import_staging_expired: { kind: 'server_import_staging_expired', token: 'tok-1' },
      data_location_busy: { kind: 'data_location_busy' },
      data_location_invalid: { kind: 'data_location_invalid', reason: 'not_empty' },
      data_location_migration_failed: {
        kind: 'data_location_migration_failed',
        reason: 'disk full',
      },
      data_location_unavailable: { kind: 'data_location_unavailable' },
      datapack_invalid: {
        kind: 'datapack_invalid',
        filename: 'MyPack.rar',
        reason: 'not_a_zip',
      },
      datapack_too_large: {
        kind: 'datapack_too_large',
        filename: 'huge.zip',
        size_bytes: 300 * 1024 * 1024,
        limit_bytes: 256 * 1024 * 1024,
      },
      vanilla_tweaks_unavailable: {
        kind: 'vanilla_tweaks_unavailable',
        mc_version: '1.12.2',
      },
      // The message is the upstream service's own wording, kept verbatim.
      vanilla_tweaks_build_failed: {
        kind: 'vanilla_tweaks_build_failed',
        message: 'no packs selected',
      },
      vanilla_tweaks_bundle_too_large: {
        kind: 'vanilla_tweaks_bundle_too_large',
        size_bytes: 3 * 1024 * 1024 * 1024,
        limit_bytes: 2 * 1024 * 1024 * 1024,
      },
      // `reason` is a nested tagged enum (`FormatError`), the first exported
      // type in this crate to carry another exported tagged enum as a field.
      // The sample uses the specifier the validator exists to catch: `%d` is
      // not valid Minecraft syntax, and a mismatch renders the raw template
      // in game rather than throwing, so nothing but this check would notice.
      l10n_translation_invalid: {
        kind: 'l10n_translation_invalid',
        key: 'item.create.wrench',
        reason: { kind: 'unsupported_specifier', specifier: 'd' },
      },
      l10n_format_unknown: { kind: 'l10n_format_unknown', mc_version: '1.21.1' },
      l10n_format_too_old: { kind: 'l10n_format_too_old', mc_version: '1.12.2' },
      // The IPC-boundary guard added alongside `l10n_translation_invalid`'s
      // reuse in `l10n::pack::build`: a `namespace`/`lang` that would corrupt
      // a composed zip-entry path if persisted verbatim, refused before it
      // ever reaches the override store.
      l10n_namespace_invalid: { kind: 'l10n_namespace_invalid', namespace: '../../evil' },
      l10n_lang_invalid: { kind: 'l10n_lang_invalid', lang: '../../evil' },
      l10n_prefill_key_missing: { kind: 'l10n_prefill_key_missing', provider: 'anthropic' },
      // `details` carries a provider's raw error body, which can echo the API
      // key back. The sample puts a key-shaped string there so the transport
      // policy assertion above proves it never reaches the rendered message.
      l10n_prefill_provider: {
        kind: 'l10n_prefill_provider',
        provider: 'anthropic',
        status: 401,
        details: 'invalid x-api-key: sk-ant-LEAKED-SECRET',
      },
      l10n_prefill_busy: { kind: 'l10n_prefill_busy' },
      // The share bundle is hostile input — a file a stranger sent. Its
      // rejection reason is a nested tagged enum (`BundleError`), so this
      // sample only proves the outer sentence renders; the nested clause is
      // pinned by the focused `schema_too_new` case below, because the
      // `startsWith('errors.')` assertion here only inspects the START of the
      // string and would miss an unresolved key interpolated mid-sentence.
      // `looks_like_resource_pack` is deliberately unused by this generic
      // formatter — the import dialog renders the richer hint for that case.
      l10n_share_bundle_invalid: {
        kind: 'l10n_share_bundle_invalid',
        error: { kind: 'no_metadata', looks_like_resource_pack: true },
      },
      l10n_share_prefill_active: { kind: 'l10n_share_prefill_active' },
      l10n_share_dest_reserved: { kind: 'l10n_share_dest_reserved' },
      l10n_share_nothing_to_export: { kind: 'l10n_share_nothing_to_export' },
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
      // Policy: transport variants never leak the raw url or transport detail.
      if (ERROR_CLASS[sample.kind] === 'transport') {
        if ('url' in sample && sample.url) expect(msg).not.toContain(sample.url);
        if ('details' in sample && sample.details) expect(msg).not.toContain(sample.details);
      }
    });

    it('has a distinct sample for every Error variant (Record completeness)', () => {
      // Both `samples` and `ERROR_CLASS` are `Record<IpcError['kind'], …>`, so
      // the build fails if either omits a `kind`. The runtime complement the
      // type system does NOT catch is a *duplicate* key in a literal, which
      // collapses two entries into one and shrinks that object's key set. We
      // catch it without a magic number by cross-checking the two independent
      // literals against each other: a duplicate in one makes its key set a
      // strict subset of the other's. Adding a new variant needs no edit here.
      const sampleKeys = Object.keys(samples).sort();
      const classKeys = Object.keys(ERROR_CLASS).sort();
      expect(sampleKeys).toEqual(classKeys);
    });

    it('classifies every Error variant (ERROR_CLASS completeness)', () => {
      // Every sampled variant resolves to a class (no `undefined`), proving
      // ERROR_CLASS covers the full variant set the samples exercise — the
      // runtime complement to the compile-time Record check, with no count to
      // bump when variants change.
      for (const kind of Object.keys(samples) as IpcError['kind'][]) {
        expect(ERROR_CLASS[kind]).toBeDefined();
      }
    });

    const opaqueKinds = (Object.keys(samples) as IpcError['kind'][]).filter(
      (k) => ERROR_CLASS[k] === 'opaque',
    );

    it.each(opaqueKinds)('opaque %s truncates a long detail and points at Logs', (kind) => {
      const base = samples[kind];
      // Most opaque variants carry the free-form text in `details`;
      // servers_dat_parse uses `reason`. Override whichever this variant renders.
      const longField =
        'details' in base ? { details: 'x'.repeat(200) } : { reason: 'x'.repeat(200) };
      const sample = { ...base, ...longField } as IpcError;
      const msg = formatError(sample);
      expect(msg).toContain('… (open Logs for full text)');
      expect(msg).not.toContain('x'.repeat(200));
    });
  });

  it('truncates a long java_spawn detail + points at Logs', () => {
    const msg = formatError({ kind: 'java_spawn', details: 'x'.repeat(200) });
    expect(msg).toContain('… (open Logs for full text)');
    expect(msg).not.toContain('x'.repeat(200));
  });

  // The nested `BundleError` is the only part of a share rejection the sample
  // sweep cannot vouch for: it lands mid-sentence, past the `startsWith`
  // check. Pinning the interpolated case proves two things at once — the
  // reason resolved to real copy rather than a raw `errors.…` key, and its
  // numeric arg was passed as a number (ICU rejects a string here, which would
  // surface as the argument dropping out of the rendered sentence).
  it('renders a share bundle rejection with its interpolated schema version', () => {
    const sample: IpcError = {
      kind: 'l10n_share_bundle_invalid',
      error: { kind: 'schema_too_new', found: 9 },
    };
    const msg = formatError(sample);
    expect(msg).toBe(
      "Couldn't read the translations file: it was made by a newer Lucerna (format 9)",
    );
    // Not raw JSON, and no unresolved key leaked into the middle of the text.
    expect(msg).not.toBe(JSON.stringify(sample));
    expect(msg).not.toContain('errors.');
    expect(msg).not.toContain('schema_too_new');
  });

  it('maps the adopt-validation reasons to human clauses', () => {
    expect(formatError({ kind: 'data_location_invalid', reason: 'not_a_data_root' })).toContain(
      "doesn't contain Lucerna data",
    );
    expect(formatError({ kind: 'data_location_invalid', reason: 'not_writable' })).toContain(
      "can't be written to",
    );
  });

  describe('withDetailTail', () => {
    beforeAll(() => locale.set('en'));

    it('returns the headline unchanged when there is no detail', () => {
      expect(withDetailTail('Headline', null)).toBe('Headline');
      expect(withDetailTail('Headline', '')).toBe('Headline');
    });

    it('appends a short detail after a colon, with no Logs hint', () => {
      expect(withDetailTail('Headline', 'boom')).toBe('Headline: boom');
    });

    it('truncates a long detail at 120 code points and appends the Logs hint', () => {
      const msg = withDetailTail('Headline', 'x'.repeat(200));
      expect(msg).toContain('x'.repeat(120));
      expect(msg).not.toContain('x'.repeat(121));
      expect(msg).toContain('… (open Logs for full text)');
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
