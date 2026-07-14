// tests/server-properties-editor.test.ts
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ServerPropertiesEditor from '$lib/servers/settings/ServerPropertiesEditor.svelte';

const { serverReadProperties, serverWriteProperties } = vi.hoisted(() => ({
  serverReadProperties: vi.fn(),
  serverWriteProperties: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { serverReadProperties, serverWriteProperties },
}));

describe('ServerPropertiesEditor', () => {
  beforeEach(() => {
    locale.set('en');
    serverReadProperties.mockReset();
    serverWriteProperties.mockReset();
  });

  it('prefills a curated field from the file', async () => {
    serverReadProperties.mockResolvedValue({ status: 'ok', data: 'view-distance=12\n' });
    render(ServerPropertiesEditor, { props: { serverId: 'srv-1', running: false } });
    const input = (await screen.findByLabelText('View distance')) as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe('12'));
  });

  it('omits an unchanged absent default but writes a changed one', async () => {
    serverReadProperties.mockResolvedValue({ status: 'ok', data: 'motd=Hi\n' });
    serverWriteProperties.mockResolvedValue({ status: 'ok', data: null });
    render(ServerPropertiesEditor, { props: { serverId: 'srv-1', running: false } });

    const view = (await screen.findByLabelText('View distance')) as HTMLInputElement;
    await fireEvent.input(view, { target: { value: '16' } });
    await fireEvent.click(screen.getByTestId('properties-save'));

    await waitFor(() => expect(serverWriteProperties).toHaveBeenCalled());
    const written = serverWriteProperties.mock.calls[0][1] as string;
    expect(written).toContain('view-distance=16');
    expect(written).toContain('motd=Hi'); // untouched present key preserved
    expect(written).not.toContain('difficulty='); // absent + default → omitted
  });

  it('filters via search', async () => {
    serverReadProperties.mockResolvedValue({ status: 'ok', data: '' });
    render(ServerPropertiesEditor, { props: { serverId: 'srv-1', running: false } });
    await screen.findByLabelText('View distance');
    const search = screen.getByPlaceholderText('Search properties…');
    await fireEvent.input(search, { target: { value: 'nether' } });
    await waitFor(() => expect(screen.queryByLabelText('View distance')).toBeNull());
    expect(screen.getByLabelText('Allow Nether')).toBeTruthy();
  });

  it('surfaces unknown file keys in the Other group', async () => {
    serverReadProperties.mockResolvedValue({ status: 'ok', data: 'my-custom-key=abc\n' });
    render(ServerPropertiesEditor, { props: { serverId: 'srv-1', running: false } });
    await waitFor(() => expect(screen.getByDisplayValue('abc')).toBeTruthy());
  });

  it('only-changed filter hides keys still at their default', async () => {
    serverReadProperties.mockResolvedValue({ status: 'ok', data: 'view-distance=12\n' });
    render(ServerPropertiesEditor, { props: { serverId: 'srv-1', running: false } });
    await screen.findByLabelText('View distance');
    expect(screen.getByLabelText('Allow Nether')).toBeTruthy(); // at default → visible now
    await fireEvent.click(screen.getByLabelText('Only changed'));
    await waitFor(() => expect(screen.queryByLabelText('Allow Nether')).toBeNull());
    expect(screen.getByLabelText('View distance')).toBeTruthy(); // 12 ≠ default 10 → stays
  });

  it('saves an edited unknown key', async () => {
    serverReadProperties.mockResolvedValue({ status: 'ok', data: 'my-custom-key=abc\n' });
    serverWriteProperties.mockResolvedValue({ status: 'ok', data: null });
    render(ServerPropertiesEditor, { props: { serverId: 'srv-1', running: false } });
    const input = (await screen.findByLabelText('my-custom-key')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'xyz' } });
    await fireEvent.click(screen.getByTestId('properties-save'));
    await waitFor(() => expect(serverWriteProperties).toHaveBeenCalled());
    const written = serverWriteProperties.mock.calls[0][1] as string;
    expect(written).toContain('my-custom-key=xyz');
  });
});
