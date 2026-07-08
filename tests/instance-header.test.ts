import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import InstanceHeader from '$lib/overview/InstanceHeader.svelte';

const inst = {
  id: 'i1',
  name: 'Skyblock',
  mc_version: '1.21.1',
  loader: 'fabric' as const,
  loader_version: '0.16.5',
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
};

describe('InstanceHeader', () => {
  it('renders the instance name and the derived avatar letter', () => {
    const { getByText, getByTestId } = render(InstanceHeader, {
      props: { instance: inst, running: false, installing: false },
    });
    expect(getByText('Skyblock')).toBeTruthy();
    // trim(): the avatar button also contains the decorative hover-overlay
    // span, whose markup contributes whitespace-only text nodes.
    expect(getByTestId('overview-avatar').textContent?.trim()).toBe('S');
  });

  it('renders MC version, loader and memory badges', () => {
    const { getByText } = render(InstanceHeader, {
      props: { instance: inst, running: false, installing: false },
    });
    expect(getByText(/1\.21\.1/)).toBeTruthy();
    expect(getByText(/Fabric/)).toBeTruthy();
    expect(getByText(/2048 MB/)).toBeTruthy();
  });

  it('shows the ready pill when installed and idle', () => {
    const { getByTestId } = render(InstanceHeader, {
      props: { instance: inst, running: false, installing: false },
    });
    expect(getByTestId('overview-status-pill').getAttribute('data-status')).toBe('ready');
  });

  it('shows the running pill when the game is up', () => {
    const { getByTestId } = render(InstanceHeader, {
      props: { instance: inst, running: true, installing: false },
    });
    expect(getByTestId('overview-status-pill').getAttribute('data-status')).toBe('running');
  });

  it('does not show the attention-restore triangle by default', () => {
    const { queryByTestId } = render(InstanceHeader, {
      props: { instance: inst, running: false, installing: false },
    });
    expect(queryByTestId('overview-attention-restore')).toBeNull();
  });

  it('does not show the triangle when collapsed but nothing is hidden', () => {
    const { queryByTestId } = render(InstanceHeader, {
      props: {
        instance: inst,
        running: false,
        installing: false,
        attentionCollapsed: true,
        attentionCount: 0,
      },
    });
    expect(queryByTestId('overview-attention-restore')).toBeNull();
  });

  it('shows the triangle when collapsed with hidden warnings', () => {
    const { getByTestId } = render(InstanceHeader, {
      props: {
        instance: inst,
        running: false,
        installing: false,
        attentionCollapsed: true,
        attentionCount: 2,
      },
    });
    expect(getByTestId('overview-attention-restore')).toBeTruthy();
  });

  it('calls onShowAttention when the triangle is clicked', async () => {
    const onShowAttention = vi.fn();
    const { getByTestId } = render(InstanceHeader, {
      props: {
        instance: inst,
        running: false,
        installing: false,
        attentionCollapsed: true,
        attentionCount: 1,
        onShowAttention,
      },
    });
    await fireEvent.click(getByTestId('overview-attention-restore'));
    expect(onShowAttention).toHaveBeenCalledOnce();
  });
});
