import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { OptimisePlan } from '$lib/ipc/bindings';
import OptimiseDialog from '$lib/mods/OptimiseDialog.svelte';

function plan(over: Partial<OptimisePlan> = {}): OptimisePlan {
  return {
    loader_unsupported: false,
    install_count: 1,
    entries: [
      {
        key: 'sodium',
        title: 'Sodium',
        status: { status: 'will_install' },
        note: null,
        version: { source: 'modrinth', project_id: 'AANobbMI', version_id: 'x' },
        version_number: '0.6.0',
      },
      {
        key: 'lithium',
        title: 'Lithium',
        status: { status: 'already_installed' },
        note: 'single_player_tick',
        version: null,
        version_number: null,
      },
      {
        key: 'embeddium',
        title: 'Embeddium',
        status: { status: 'unavailable_for_version' },
        note: null,
        version: null,
        version_number: null,
      },
    ],
    ...over,
  };
}

describe('OptimiseDialog', () => {
  it('renders the three sections and the install button with count', () => {
    render(OptimiseDialog, {
      props: {
        plan: plan(),
        loader: 'fabric',
        mc: '1.21.1',
        installing: false,
        onConfirm: vi.fn(),
        onCancel: vi.fn(),
      },
    });
    expect(screen.getByText('Sodium')).toBeTruthy();
    expect(screen.getByText('Lithium')).toBeTruthy();
    expect(screen.getByText('Embeddium')).toBeTruthy();
    expect(screen.getByRole('button', { name: /Install 1/ })).toBeTruthy();
  });

  it('disables install when nothing to install', () => {
    render(OptimiseDialog, {
      props: {
        plan: plan({ install_count: 0, entries: [] }),
        loader: 'fabric',
        mc: '1.21.1',
        installing: false,
        onConfirm: vi.fn(),
        onCancel: vi.fn(),
      },
    });
    const btn = screen.getByRole('button', { name: /Install 0/ }) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
  });
});
