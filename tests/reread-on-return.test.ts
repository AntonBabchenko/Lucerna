// rereadOnReturn: a list re-reads when the user comes back to the window — at once when it can,
// and, when the return lands while the list is hidden or busy, as soon as it can.
import { render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import RereadOnReturnHost from './fixtures/RereadOnReturnHost.svelte';

const tick = () => new Promise((r) => setTimeout(r, 0));
const returnToWindow = () => window.dispatchEvent(new Event('focus'));

describe('rereadOnReturn', () => {
  it('reads once per return while the list is on screen and idle', async () => {
    const onReread = vi.fn();
    render(RereadOnReturnHost, { props: { onReread } });
    returnToWindow();
    returnToWindow();
    await tick();
    expect(onReread).toHaveBeenCalledTimes(2);
  });

  it('a return while hidden is read when the list shows, and only once', async () => {
    const onReread = vi.fn();
    const { rerender } = render(RereadOnReturnHost, { props: { onReread, visible: false } });
    returnToWindow();
    await tick();
    expect(onReread).not.toHaveBeenCalled();
    await rerender({ visible: true });
    expect(onReread).toHaveBeenCalledTimes(1);
    await rerender({ visible: false });
    await rerender({ visible: true });
    expect(onReread).toHaveBeenCalledTimes(1);
  });

  it('a return during a write is read when the write ends', async () => {
    const onReread = vi.fn();
    const { rerender } = render(RereadOnReturnHost, { props: { onReread, idle: false } });
    returnToWindow();
    await tick();
    expect(onReread).not.toHaveBeenCalled();
    await rerender({ idle: true });
    expect(onReread).toHaveBeenCalledTimes(1);
  });

  it('reads nothing while there is nothing to read, and remembers nothing for later', async () => {
    const onReread = vi.fn();
    const { rerender } = render(RereadOnReturnHost, { props: { onReread, active: false } });
    returnToWindow();
    await tick();
    await rerender({ active: true });
    expect(onReread).not.toHaveBeenCalled();
  });

  it('a hidden list shown with no return in between reads nothing', async () => {
    const onReread = vi.fn();
    const { rerender } = render(RereadOnReturnHost, { props: { onReread, visible: false } });
    await rerender({ visible: true });
    expect(onReread).not.toHaveBeenCalled();
  });
});
