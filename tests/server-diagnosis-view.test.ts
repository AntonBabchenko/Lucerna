import { describe, expect, it } from 'vitest';
import type { ServerDiagnosis } from '$lib/ipc/bindings';
import {
  bannerExplainsActionError,
  serverBannerEligible,
  serverDiagnosisSignature,
} from '$lib/servers/server-diagnosis-view';

function diag(overrides: Partial<ServerDiagnosis> = {}): ServerDiagnosis {
  return {
    status: 'actionable',
    diagnosis: {
      pattern_id: 'server-missing-dep',
      title: 't',
      explanation: 'e',
      recommendation: 'r',
      matched_excerpt: '',
      repair: null,
    },
    client_mods: [],
    forge_skip_count: null,
    log_signature: 'sig-1',
    server_repair: null,
    port_in_use: null,
    orphan_pid: null,
    corrupt_jar: null,
    suggested_heap_mb: null,
    conflict_mods: [],
    suggested_port: null,
    exit_code: null,
    ...overrides,
  };
}

describe('serverBannerEligible', () => {
  it('is true for an actionable, stopped diagnosis', () => {
    expect(serverBannerEligible(diag(), false)).toBe(true);
  });

  it('is false while the server is running', () => {
    expect(serverBannerEligible(diag(), true)).toBe(false);
  });

  it('is false for none / handled / missing diagnosis', () => {
    expect(serverBannerEligible(diag({ status: 'none' }), false)).toBe(false);
    expect(serverBannerEligible(diag({ status: 'handled' }), false)).toBe(false);
    expect(serverBannerEligible(diag({ diagnosis: null }), false)).toBe(false);
    expect(serverBannerEligible(null, false)).toBe(false);
    expect(serverBannerEligible(undefined, false)).toBe(false);
  });
});

describe('serverDiagnosisSignature', () => {
  it('combines pattern_id and log_signature', () => {
    expect(serverDiagnosisSignature(diag())).toBe('server-missing-dep|sig-1');
  });

  it('tolerates a null log_signature (pre-spawn diagnoses)', () => {
    expect(serverDiagnosisSignature(diag({ log_signature: null }))).toBe('server-missing-dep|');
  });

  it('differs when the pattern differs', () => {
    const withPattern = (pattern_id: string): ServerDiagnosis =>
      diag({
        diagnosis: {
          pattern_id,
          title: 't',
          explanation: 'e',
          recommendation: 'r',
          matched_excerpt: '',
          repair: null,
        },
      });
    expect(serverDiagnosisSignature(withPattern('a'))).not.toBe(
      serverDiagnosisSignature(withPattern('b')),
    );
  });

  it('differs when only the log_signature differs (same pattern, different log)', () => {
    expect(serverDiagnosisSignature(diag({ log_signature: 'x' }))).not.toBe(
      serverDiagnosisSignature(diag({ log_signature: 'y' })),
    );
  });

  it('is null when there is no diagnosis', () => {
    expect(serverDiagnosisSignature(diag({ diagnosis: null }))).toBeNull();
    expect(serverDiagnosisSignature(null)).toBeNull();
  });
});

describe('bannerExplainsActionError', () => {
  const eulaBanner = diag({
    diagnosis: {
      pattern_id: 'server-eula-not-accepted',
      title: 't',
      explanation: 'e',
      recommendation: 'r',
      matched_excerpt: '',
      repair: null,
    },
    log_signature: null,
    server_repair: 'accept_eula',
  });

  it('is true only for the refusal the banner states: a start refused for the EULA', () => {
    expect(bannerExplainsActionError(eulaBanner, { kind: 'server_eula_not_accepted' })).toBe(true);
  });

  it('is false for a refusal no banner states, whatever the banner says', () => {
    // A server refused before it starts keeps the previous run's log, so the
    // banner may describe an old crash; it never describes this refusal.
    expect(bannerExplainsActionError(diag(), { kind: 'server_saved_mc_version_missing' })).toBe(
      false,
    );
    expect(bannerExplainsActionError(eulaBanner, { kind: 'server_loader_ambiguous' })).toBe(false);
  });

  it('is false for the EULA refusal under a banner about something else', () => {
    expect(bannerExplainsActionError(diag(), { kind: 'server_eula_not_accepted' })).toBe(false);
  });

  it('is false without a diagnosis, or for a thrown (non-IPC) failure', () => {
    expect(
      bannerExplainsActionError(diag({ status: 'none', diagnosis: null }), {
        kind: 'server_eula_not_accepted',
      }),
    ).toBe(false);
    expect(bannerExplainsActionError(undefined, { kind: 'server_eula_not_accepted' })).toBe(false);
    expect(bannerExplainsActionError(eulaBanner, new Error('transport'))).toBe(false);
    expect(bannerExplainsActionError(eulaBanner, 'boom')).toBe(false);
  });
});
