import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import InstanceHeader from '$lib/overview/InstanceHeader.svelte';

const inst = {
  id: 'i1',
  name: 'Skyblock',
  mc_version: '1.21.1',
  loader: 'fabric' as const,
  loader_version: '0.16.5',
  max_heap_mb: 2048,
  extra_jvm_args: '',
  created_unix_ms: null,
  ready: true,
  mrpack_name: null,
  mrpack_version: null,
  mrpack_project_id: null,
  mrpack_source: null,
  mrpack_summary: null,
  mrpack_version_id: null,
  integrity: null,
};

describe('InstanceHeader', () => {
  it('renders the instance name and the derived avatar letter', () => {
    const { getByText, getByTestId } = render(InstanceHeader, {
      props: { instance: inst, running: false, installing: false },
    });
    expect(getByText('Skyblock')).toBeTruthy();
    expect(getByTestId('overview-avatar').textContent).toBe('S');
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
});
