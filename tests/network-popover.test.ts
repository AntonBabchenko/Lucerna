import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import NetworkPopover from '$lib/network/NetworkPopover.svelte';

// IPC stubs: open=true triggers an effect that calls both commands, so
// both must resolve to non-rejecting promises or the component throws
// at render time.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    networkActivity: vi.fn().mockResolvedValue([]),
    networkAuditViolations: vi.fn().mockResolvedValue([]),
  },
}));

describe('NetworkPopover', () => {
  it('renders nothing when open is false', () => {
    const { queryByText } = render(NetworkPopover, { props: { open: false } });
    expect(queryByText('Network Activity')).toBeFalsy();
  });

  it('renders the popover header when open is true', async () => {
    const { findByText } = render(NetworkPopover, { props: { open: true } });
    expect(await findByText('Network Activity')).toBeTruthy();
  });

  it('clicking the backdrop closes the popover', async () => {
    const { findByLabelText, queryByText } = render(NetworkPopover, {
      props: { open: true },
    });
    const backdrop = await findByLabelText('Close Network Activity');
    await fireEvent.click(backdrop);
    await waitFor(() => {
      expect(queryByText('Network Activity')).toBeFalsy();
    });
  });

  it('clicking the × button closes the popover', async () => {
    const { findByLabelText, queryByText } = render(NetworkPopover, {
      props: { open: true },
    });
    // The × button uses aria-label="Close" (exact). The backdrop uses
    // aria-label="Close Network Activity" — exact:true here disambiguates.
    const xButton = await findByLabelText('Close', { exact: true });
    await fireEvent.click(xButton);
    await waitFor(() => {
      expect(queryByText('Network Activity')).toBeFalsy();
    });
  });

  it('Escape on the backdrop closes the popover', async () => {
    const { findByLabelText, queryByText } = render(NetworkPopover, {
      props: { open: true },
    });
    const backdrop = await findByLabelText('Close Network Activity');
    await fireEvent.keyDown(backdrop, { key: 'Escape' });
    await waitFor(() => {
      expect(queryByText('Network Activity')).toBeFalsy();
    });
  });
});
