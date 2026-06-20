import { describe, expect, it } from 'vitest';
import type { ServerWithStatus_Serialize } from '$lib/ipc/bindings';
import {
  diagnosisStatusOf,
  isCrashed,
  isDiagnosisActionable,
  lastExitCodeOf,
} from '$lib/servers/runtime-extra';

// Build a server with the documented C1 fields layered on (they aren't on the
// generated type yet — the shim reads them defensively).
function makeServer(over: Record<string, unknown> = {}): ServerWithStatus_Serialize {
  return {
    id: 'srv',
    name: 'srv',
    mc_version: '1.21',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: null,
    eula_accepted: true,
    created_from_instance: null,
    running: false,
    pid: null,
    port: 25565,
    upload: null,
    upload_password_set: false,
    last_exit_code: null,
    diagnosis_status: 'none',
    ...over,
  } as ServerWithStatus_Serialize;
}

describe('runtime-extra: diagnosis (#4 / C1)', () => {
  it('defaults to none when the field is absent (pre-merge bindings)', () => {
    expect(diagnosisStatusOf(makeServer())).toBe('none');
    expect(isDiagnosisActionable(makeServer())).toBe(false);
  });

  it('reads an actionable diagnosis', () => {
    const s = makeServer({ diagnosis_status: 'actionable' });
    expect(diagnosisStatusOf(s)).toBe('actionable');
    expect(isDiagnosisActionable(s)).toBe(true);
  });

  it('advisory and handled are not actionable', () => {
    expect(isDiagnosisActionable(makeServer({ diagnosis_status: 'advisory' }))).toBe(false);
    expect(isDiagnosisActionable(makeServer({ diagnosis_status: 'handled' }))).toBe(false);
  });
});

describe('runtime-extra: crash state (#18 / C1)', () => {
  it('a running server is never "crashed"', () => {
    expect(isCrashed(makeServer({ running: true, last_exit_code: 1 }))).toBe(false);
  });

  it('a stopped server with a non-zero exit code crashed', () => {
    expect(isCrashed(makeServer({ running: false, last_exit_code: 1 }))).toBe(true);
  });

  it('a clean exit (null) is not a crash', () => {
    expect(isCrashed(makeServer({ running: false, last_exit_code: null }))).toBe(false);
    expect(lastExitCodeOf(makeServer())).toBeNull();
  });
});
