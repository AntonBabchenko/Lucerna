import { fireEvent, render } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import FileDropzone from '../src/lib/mods/FileDropzone.svelte';

describe('FileDropzone', () => {
  afterEach(async () => {
    const { dragActive } = await import('$lib/settings/state.svelte');
    dragActive.value = false;
  });

  it('renders its label and calls onClick when clicked', async () => {
    const onClick = vi.fn();
    const { getByTestId } = render(FileDropzone, {
      props: { label: 'Drop a .jar here', onClick },
    });
    await fireEvent.click(getByTestId('file-dropzone'));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('shows the disabled label and does not fire onClick when disabled', async () => {
    const onClick = vi.fn();
    const { getByTestId } = render(FileDropzone, {
      props: {
        label: 'Drop a .jar here',
        disabled: true,
        disabledLabel: 'Pick an instance first',
        onClick,
      },
    });
    const zone = getByTestId('file-dropzone');
    expect(zone.textContent).toContain('Pick an instance first');
    await fireEvent.click(zone);
    expect(onClick).not.toHaveBeenCalled();
  });

  it('reflects the dragActive rune in its highlight class', async () => {
    const { getByTestId } = render(FileDropzone, {
      props: { label: 'Drop a .jar here', onClick: () => {} },
    });
    const { dragActive } = await import('$lib/settings/state.svelte');
    expect(getByTestId('file-dropzone').className).not.toContain('bg-accent-soft');
    dragActive.value = true;
    await tick();
    expect(getByTestId('file-dropzone').className).toContain('bg-accent-soft');
  });

  it('does not show the drag highlight while disabled', async () => {
    const { getByTestId } = render(FileDropzone, {
      props: { label: 'Drop a .jar here', disabled: true, onClick: () => {} },
    });
    const { dragActive } = await import('$lib/settings/state.svelte');
    dragActive.value = true;
    await tick();
    // A disabled dropzone stays muted even mid-drag — the highlight is
    // gated on `!disabled`.
    expect(getByTestId('file-dropzone').className).not.toContain('bg-accent-soft');
  });

  it('is the full box by default and paints no overlay', () => {
    const { getByTestId, queryByTestId } = render(FileDropzone, {
      props: { label: 'Drop', onClick: () => {} },
    });
    expect(getByTestId('file-dropzone').dataset.variant).toBe('full');
    expect(queryByTestId('file-dropzone-overlay')).toBeNull();
  });

  // The strip grows into an overlay over its host while a file is dragged. The overlay is
  // decorative — the window-level listener takes the drop — so it can never swallow a pointer
  // event or reach a screen reader; the strip stays the file-picker button, by keyboard too.
  it('a strip paints a decorative overlay with its drag label while a file is dragged', async () => {
    const { dragActive } = await import('$lib/settings/state.svelte');
    const onClick = vi.fn();
    const { getByTestId } = render(FileDropzone, {
      props: {
        label: 'Drop a .jar here',
        dragLabel: 'Drop to add to “Test”',
        variant: 'strip',
        onClick,
      },
    });
    const overlay = getByTestId('file-dropzone-overlay');
    expect(overlay.className).toContain('opacity-0');
    expect(overlay.className).toContain('pointer-events-none');
    expect(overlay.getAttribute('aria-hidden')).toBe('true');
    dragActive.value = true;
    await tick();
    expect(overlay.className).toContain('opacity-100');
    expect(overlay.textContent).toContain('Drop to add to “Test”');
    expect(getByTestId('file-dropzone').className).toContain('bg-accent-soft');
    dragActive.value = false;
    await tick();
    expect(overlay.className).toContain('opacity-0');
    const strip = getByTestId('file-dropzone');
    expect(strip.getAttribute('role')).toBe('button');
    await fireEvent.keyDown(strip, { key: 'Enter' });
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('a disabled strip keeps its overlay hidden mid-drag', async () => {
    const { dragActive } = await import('$lib/settings/state.svelte');
    const { getByTestId } = render(FileDropzone, {
      props: { label: 'x', variant: 'strip', disabled: true, onClick: () => {} },
    });
    dragActive.value = true;
    await tick();
    expect(getByTestId('file-dropzone-overlay').className).toContain('opacity-0');
  });
});
