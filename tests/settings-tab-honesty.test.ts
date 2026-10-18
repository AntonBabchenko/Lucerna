// v0.28 design review, group A: the instance Settings tab and the sidebar say what is true and
// keep the keyboard user's place.
import { render, screen } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import en from '$lib/i18n/locales/en.json';
import ru from '$lib/i18n/locales/ru.json';
import InstanceLaunchSection from '$lib/instances/settings/InstanceLaunchSection.svelte';
import type { InstanceWithStatus } from '$lib/ipc/bindings';
import { focusInstancePickerIfLost } from '$lib/layout/picker-focus';
import Sidebar from '$lib/layout/Sidebar.svelte';
import { newLayerId } from '$lib/ui/layer-stack.svelte';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import { trapFocus } from '$lib/ui/trap-focus';
import { revealTooltip } from './test-utils/reveal-tooltip';

vi.mock('$lib/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/ipc/bindings')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      beginMicrosoftSignin: vi.fn().mockResolvedValue({ status: 'ok', data: {} }),
      instanceMemoryBounds: vi.fn().mockResolvedValue({
        min_mb: 1024,
        max_mb: 8192,
        default_mb: 2048,
        recommended_max_mb: 8192,
        step_mb: 256,
        ram_known: false,
      }),
    },
  };
});

function instance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Survival',
    mc_version: '1.20.4',
    loader: 'fabric',
    loader_version: '0.15.0',
    max_heap_mb: 2048,
    min_heap_mb: null,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: true,
    has_icon: false,
    mrpack_name: null,
    mrpack_version: null,
    mrpack_project_id: null,
    mrpack_source: null,
    mrpack_summary: null,
    mrpack_version_id: null,
    integrity: null,
    imported_from: null,
    created_from_server: null,
    java: null,
    ...over,
  };
}

function sidebarProps(over: Record<string, unknown> = {}) {
  return {
    accounts: [],
    activeAccount: null,
    instances: [] as InstanceWithStatus[],
    activeInstance: null,
    onSelectAccount: vi.fn(),
    onRemoveAccount: vi.fn(),
    onOpenCosmetics: vi.fn(),
    onAddOffline: vi.fn(),
    onSelectInstance: vi.fn(),
    onCreateInstance: vi.fn(),
    onOpenMods: vi.fn(),
    onOpenLogs: vi.fn(),
    onOpenModpacks: vi.fn(),
    onOpenLauncherImport: vi.fn(),
    running: null,
    installing: false,
    onPlay: vi.fn(),
    onStop: vi.fn(),
    onInstall: vi.fn(),
    ...over,
  };
}

beforeAll(() => locale.set('en'));
afterEach(() => {
  hideTooltip();
  document.body.innerHTML = '';
});

describe('the tab’s tour says what Delete does', () => {
  it('no longer asks to type a word, in either language', () => {
    expect(en.onboarding.contextual.instanceSettings.danger.body).not.toMatch(/type/i);
    expect(ru.onboarding.contextual.instanceSettings.danger.body).not.toMatch(/ввод/i);
  });
});

describe('an instance list that could not be read is not «no instances»', () => {
  it('says it could not be read and offers Try again, not Create', async () => {
    const onRetryInstances = vi.fn();
    render(Sidebar, {
      props: sidebarProps({ instancesListError: 'Access is denied.', onRetryInstances }),
    });
    expect(screen.getByText(/Couldn't read your instances\. Access is denied\./)).toBeTruthy();
    expect(screen.queryByText(/No instances yet/)).toBeNull();
    expect(screen.queryByRole('button', { name: /Create/ })).toBeNull();
    screen.getByRole('button', { name: 'Try again' }).click();
    expect(onRetryInstances).toHaveBeenCalledOnce();
  });
});

describe('the dropdown’s status glyphs explain themselves', () => {
  it('the integrity triangle has a tooltip', () => {
    const inst = instance({
      integrity: { healthy: false, problem_count: 3 } as never,
    });
    render(Sidebar, {
      props: sidebarProps({ instances: [inst], activeInstance: inst }),
    });
    const glyph = screen.getByRole('img', { name: /3 integrity problems/i });
    revealTooltip(glyph.parentElement as Element);
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toMatch(/3 integrity problems/i);
  });
});

describe('while the game runs, Launch says when its changes apply', () => {
  it('shows the next-start line only while running', () => {
    const { rerender } = render(InstanceLaunchSection, {
      props: { instance: instance(), running: true, onChanged: vi.fn() },
    });
    expect(screen.getByTestId('launch-applies-next-start').textContent).toMatch(
      /apply the next time the game starts/,
    );
    rerender({ instance: instance(), running: false, onChanged: vi.fn() });
    expect(screen.queryByTestId('launch-applies-next-start')).toBeNull();
  });
});

describe('the keyboard user keeps a place', () => {
  it('a lost focus goes to the instance picker; a focus elsewhere is left alone', async () => {
    document.body.innerHTML =
      '<div data-tour="instance-picker"><button role="combobox">Survival</button></div><button id="other">Other</button>';
    const picker = document.querySelector<HTMLElement>('[role="combobox"]') as HTMLElement;
    (document.activeElement as HTMLElement | null)?.blur();
    await focusInstancePickerIfLost();
    expect(document.activeElement).toBe(picker);
    const other = document.getElementById('other') as HTMLElement;
    other.focus();
    await focusInstancePickerIfLost();
    expect(document.activeElement).toBe(other);
  });

  it('closing a dialog gives the focus back without scrolling', () => {
    const opener = document.createElement('button');
    document.body.append(opener);
    opener.focus();
    const panel = document.createElement('div');
    panel.append(document.createElement('button'));
    document.body.append(panel);
    const trap = trapFocus(panel, newLayerId('test'));
    const focus = vi.spyOn(opener, 'focus');
    trap.destroy?.();
    expect(focus).toHaveBeenCalledWith({ preventScroll: true });
  });
});
