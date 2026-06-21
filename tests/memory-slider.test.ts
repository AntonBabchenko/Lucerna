import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
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

  it('fires onInput with the parsed integer MB when dragged', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);
    const onInput = vi.fn();

    render(MemorySlider, { props: { valueMb: 6144, onInput } });

    const slider = screen.getByRole('slider') as HTMLInputElement;
    await waitFor(() => expect(slider.max).toBe('32768'));
    await fireEvent.input(slider, { target: { value: '8192' } });

    expect(onInput).toHaveBeenCalledWith(8192);
  });

  it('warns when the chosen heap exceeds the recommendation and RAM is known', async () => {
    mockLoad.mockResolvedValue(RAM_32GB);

    // 30 GB > 24 GB recommended → warning, naming the 24.0 GB recommendation.
    render(MemorySlider, { props: { valueMb: 30720, onInput: vi.fn() } });

    expect(await screen.findByText(/24\.0 GB/)).toBeTruthy();
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
});
