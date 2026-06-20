import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { ServerDiagnosis } from '$lib/ipc/bindings';

// The inline `actionError` in ServerManageView is the UNCLASSIFIED fallback:
// when a start failure also produced a rich diagnosis banner for the server,
// the banner owns the message and the inline <p> must NOT duplicate it.
// These tests pin that gate (Task 6 of the server launch-error polish).

// Heavy children are stubbed so the view mounts in jsdom without their deps.
vi.mock('$lib/servers/ServerConsole.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerGeneralSettings.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerSettings.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerMods.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerHostingTab.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerToInstanceDialog.svelte', () => ({ default: stubComponent() }));
// The diagnosis banner renders nothing relevant to these assertions; stub it so
// its own store reads don't interfere with the inline-error assertion.
vi.mock('$lib/servers/ServerDiagnosisBanner.svelte', () => ({ default: stubComponent() }));

// A minimal no-op Svelte 5 component for child stubs. Svelte 5 mounts a
// component by CALLING it (not `new`), so a plain function that renders
// nothing is a valid empty child.
function stubComponent() {
  return function noopComponent() {
    return {};
  };
}

// serverStart resolves to an error so start() sets actionError + diagnoses.
const serverStart = vi.fn();
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverStart: (...args: unknown[]) => serverStart(...args),
    serverStop: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    serverRestart: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {},
}));

// formatError just echoes a readable message for the assertion.
vi.mock('$lib/ipc/format-error', () => ({
  formatError: () => 'boom',
}));

const mockDiagnoses: Record<string, ServerDiagnosis | undefined> = {};
const diagnose = vi.fn().mockResolvedValue(undefined);

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [{ id: 'srv-1', name: 'My server', running: false, port: 25565 }];
    },
    running: (_id: string) => false,
    diagnose: (...args: unknown[]) => diagnose(...args),
    diagnosisFor: (id: string) => mockDiagnoses[id],
    refresh: vi.fn().mockResolvedValue(undefined),
  },
}));

import ServerManageView from '$lib/servers/ServerManageView.svelte';

const noop = () => {};

describe('ServerManageView inline action error gate', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => {
    for (const k of Object.keys(mockDiagnoses)) delete mockDiagnoses[k];
    serverStart.mockReset();
    diagnose.mockClear();
  });

  it('does NOT show the inline action error when the banner has a diagnosis', async () => {
    // A classified diagnosis exists → banner owns the message.
    mockDiagnoses['srv-1'] = {
      status: 'actionable',
      diagnosis: {
        pattern_id: 'server-port-in-use',
        title: '',
        explanation: '',
        recommendation: '',
        matched_excerpt: '',
        repair: null,
      },
      client_mods: [],
      forge_skip_count: null,
      log_signature: null,
      server_repair: 'change_port',
      port_in_use: 25565,
      orphan_pid: null,
      corrupt_jar: null,
      suggested_heap_mb: null,
      conflict_mods: [],
    };
    serverStart.mockResolvedValue({ status: 'error', error: 'x' });

    render(ServerManageView, {
      props: { serverId: 'srv-1', onBack: noop, onInstanceCreated: noop },
    });
    // Trigger start() → it sets actionError then diagnoses.
    await fireEvent.click(screen.getByText('Start'));

    await waitFor(() => expect(serverStart).toHaveBeenCalled());
    expect(screen.queryByTestId('server-action-error')).toBeNull();
  });

  it('shows the inline action error when there is NO banner diagnosis (fallback)', async () => {
    // No diagnosis for this server → the inline fallback is the only surface.
    serverStart.mockResolvedValue({ status: 'error', error: 'x' });

    render(ServerManageView, {
      props: { serverId: 'srv-1', onBack: noop, onInstanceCreated: noop },
    });
    await fireEvent.click(screen.getByText('Start'));

    await waitFor(() => expect(screen.getByTestId('server-action-error')).toBeTruthy());
    expect(screen.getByTestId('server-action-error').textContent).toContain('boom');
  });
});
