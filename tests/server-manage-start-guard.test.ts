import { render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';

// Heavy children are stubbed so the view mounts in jsdom without their deps.
vi.mock('$lib/servers/ServerConsole.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerGeneralSettings.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerSettings.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerMods.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerHostingTab.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerToInstanceDialog.svelte', () => ({ default: stubComponent() }));
vi.mock('$lib/servers/ServerDiagnosisBanner.svelte', () => ({ default: stubComponent() }));

function stubComponent() {
  return function noopComponent() {
    return {};
  };
}

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    serverStart: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    serverStop: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    serverRestart: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
  events: {},
}));

vi.mock('$lib/ipc/format-error', () => ({
  formatError: () => 'error',
}));

let mockUploading = false;

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get list() {
      return [{ id: 'srv-1', name: 'My server', running: false, port: 25565 }];
    },
    running: (_id: string) => false,
    diagnose: vi.fn().mockResolvedValue(undefined),
    diagnosisFor: (_id: string) => undefined,
    refresh: vi.fn().mockResolvedValue(undefined),
    isUploading: (_id: string) => mockUploading,
  },
}));

import ServerManageView from '$lib/servers/ServerManageView.svelte';

const noop = () => {};

describe('ServerManageView Start/Restart guard during upload', () => {
  beforeAll(() => locale.set('en'));

  it('Start button is disabled when upload is in progress', () => {
    mockUploading = true;
    render(ServerManageView, {
      props: { serverId: 'srv-1', onBack: noop, onInstanceCreated: noop },
    });
    const startBtn = screen.getByText('Start').closest('button');
    expect(startBtn?.disabled).toBe(true);
  });

  it('Start button is enabled when no upload is in progress', () => {
    mockUploading = false;
    render(ServerManageView, {
      props: { serverId: 'srv-1', onBack: noop, onInstanceCreated: noop },
    });
    const startBtn = screen.getByText('Start').closest('button');
    expect(startBtn?.disabled).toBe(false);
  });
});
