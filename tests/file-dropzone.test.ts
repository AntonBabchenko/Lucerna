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
});
