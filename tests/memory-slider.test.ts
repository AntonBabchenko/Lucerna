import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import { formatHeapLabel } from '$lib/instances/heap';
import MemorySlider from '$lib/instances/MemorySlider.svelte';

const { mockLoad } = vi.hoisted(() => ({ mockLoad: vi.fn() }));

vi.mock('$lib/instances/memory-bounds', () => ({
  FALLBACK_MEMORY_BOUNDS: {
    min_mb: 1024,
    max_mb: 8192,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  },
  loadMemoryBounds: mockLoad,
}));

// A 32 GB machine: full-RAM ceiling, 75 % recommendation, RAM known.
const RAM_32GB = {
  min_mb: 1024,
  max_mb: 32768,
  recommended_max_mb: 24576,
  step_mb: 512,
  ram_known: true,
};

describe('MemorySlider', () => {
  beforeAll(() => locale.set('en'));

  it('renders the range with min/max/step from the adaptive bounds', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);

    render(MemorySlider, { props: { valueMb: 6144, onInput: vi.fn() } });

    const slider = screen.getByRole('slider') as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('32768'));
    expect(slider.min).toBe('1024');
    expect(slider.step).toBe('512');
  });

  it('exposes a human-readable aria-valuetext that tracks valueMb', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);
    const { rerender } = render(MemorySlider, { props: { valueMb: 4096, onInput: vi.fn() } });

    const slider = screen.getByRole('slider') as HTMLInputElement;
    expect(slider.getAttribute('aria-valuetext')).toBe(formatHeapLabel(4096));

    await rerender({ valueMb: 8192, onInput: vi.fn() });
    expect(slider.getAttribute('aria-valuetext')).toBe(formatHeapLabel(8192));
  });

  it('fires onInput with the parsed integer MB when dragged', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);
    const onInput = vi.fn();

    render(MemorySlider, { props: { valueMb: 6144, onInput } });

    const slider = screen.getByRole('slider') as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('32768'));
    await fireEvent.input(slider, { target: { value: '8192' } });

    expect(onInput).toHaveBeenCalledWith(8192);
  });

  it('fires onCommit on change (release) with the parsed MB, not on input', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);
    const onInput = vi.fn();
    const onCommit = vi.fn();

    render(MemorySlider, { props: { valueMb: 6144, onInput, onCommit } });

    const slider = screen.getByRole('slider') as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('32768'));

    await fireEvent.input(slider, { target: { value: '8192' } });
    expect(onCommit).not.toHaveBeenCalled();

    await fireEvent.change(slider, { target: { value: '8192' } });
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith(8192);
  });

  it('treats change as a safe no-op when onCommit is omitted', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);

    render(MemorySlider, { props: { valueMb: 6144, onInput: vi.fn() } });

    const slider = screen.getByRole('slider') as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('32768'));
    // Server surfaces omit onCommit; releasing the drag must not throw.
    await fireEvent.change(slider, { target: { value: '8192' } });
    expect(screen.getByRole('slider')).toBeTruthy();
  });

  it('warns when the chosen heap exceeds the recommendation and RAM is known', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);

    // 30 GB > 24 GB recommended → warning, naming the 24.0 GB recommendation.
    // Match the warning text specifically: the recommended-max marker (PR8) also
    // renders an sr-only "Recommended maximum: 24.0 GB", so a bare /24.0 GB/
    // would match two elements.
    render(MemorySlider, { props: { valueMb: 30720, onInput: vi.fn() } });

    expect(await screen.findByText(/Allocating more than 24\.0 GB/)).toBeTruthy();
  });

  it('does not warn when RAM is unknown, even for a large heap', async () => {
    mockLoad.mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    });

    render(MemorySlider, { props: { valueMb: 99999, onInput: vi.fn() } });

    // Let the bounds settle, then confirm no warning copy is present.
    const slider = screen.getByRole('slider') as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('8192'));
    expect(screen.queryByText(/may leave little memory/)).toBeNull();
  });

  it('reserves spacer height and forwards id/class when no warning shows (instance detail)', async () => {
    mockLoad.mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    });

    const { container } = render(MemorySlider, {
      props: {
        valueMb: 4096,
        onInput: vi.fn(),
        id: 'detail-memory',
        class: 'mb-1',
        warnClass: 'mb-3',
        reserveWarnSpace: true,
      },
    });

    const slider = screen.getByRole('slider') as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('8192'));
    // id is forwarded so an external <label for> can target it; extra class applied.
    expect(slider.id).toBe('detail-memory');
    expect(slider.classList.contains('mb-1')).toBe(true);
    // No warning, but the spacer keeps the layout from shifting.
    expect(screen.queryByText(/may leave little memory/)).toBeNull();
    expect(container.querySelector('div.mb-3')).toBeTruthy();
  });

  it('applies warnClass to the warning when reserving space and above recommendation', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);

    render(MemorySlider, {
      props: { valueMb: 30720, onInput: vi.fn(), warnClass: 'mb-3', reserveWarnSpace: true },
    });

    const warning = await screen.findByText(/may leave little memory/);
    expect(warning.classList.contains('mb-3')).toBe(true);
  });

  it('numeric field fires onInput live and onCommit (clamped + step-rounded) on change', async () => {
    mockLoad.mockResolvedValue(RAM_32GB); // min 1024, max 32768, step 512
    const onInput = vi.fn();
    const onCommit = vi.fn();
    render(MemorySlider, { props: { valueMb: 4096, onInput, onCommit } });

    await waitFor(() => expect((screen.getByRole('slider') as HTMLInputElement).max).toBe('32768'));
    const num = screen.getByRole('spinbutton') as HTMLInputElement;

    await fireEvent.input(num, { target: { value: '8000' } });
    expect(onInput).toHaveBeenLastCalledWith(8000); // live, unclamped
    expect(onCommit).not.toHaveBeenCalled();

    await fireEvent.change(num, { target: { value: '8000' } });
    expect(onCommit).toHaveBeenLastCalledWith(8192); // 8000 → nearest 512 step

    await fireEvent.change(num, { target: { value: '99999' } });
    expect(onCommit).toHaveBeenLastCalledWith(32768); // clamped to max

    await fireEvent.change(num, { target: { value: '10' } });
    expect(onCommit).toHaveBeenLastCalledWith(1024); // clamped to min
  });

  it('renders min/max endpoint labels from the adaptive bounds', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);
    render(MemorySlider, { props: { valueMb: 4096, onInput: vi.fn() } });

    expect(await screen.findByText(formatHeapLabel(32768))).toBeTruthy();
    expect(screen.getByText(formatHeapLabel(1024))).toBeTruthy();
  });

  it('shows the recommended-max marker only when RAM is known', async () => {
    mockLoad.mockResolvedValue(RAM_32GB); // ram_known: true, recommended 24576 < max
    const { unmount } = render(MemorySlider, { props: { valueMb: 4096, onInput: vi.fn() } });
    expect(await screen.findByText(/recommended maximum/i)).toBeTruthy();
    unmount();

    mockLoad.mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    });
    render(MemorySlider, { props: { valueMb: 4096, onInput: vi.fn() } });
    await waitFor(() => expect((screen.getByRole('slider') as HTMLInputElement).max).toBe('8192'));
    expect(screen.queryByText(/recommended maximum/i)).toBeNull();
  });

  it('keeps the numeric field in sync when valueMb changes', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);
    const { rerender } = render(MemorySlider, { props: { valueMb: 4096, onInput: vi.fn() } });

    const num = screen.getByRole('spinbutton') as HTMLInputElement;
    expect(num.value).toBe('4096');

    await rerender({ valueMb: 16384, onInput: vi.fn() });
    expect(num.value).toBe('16384');
  });
});
