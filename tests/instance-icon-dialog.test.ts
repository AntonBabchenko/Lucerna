import { fireEvent, render } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// InstanceIconDialog must join Modal's shared open-stack instead of
// hand-rolling its own backdrop + `<svelte:window onkeydown>` Escape handler.
// Regression: previously, opening this dialog from inside another modal (e.g.
// ManageInstancesModal) and pressing Escape closed BOTH dialogs, because this
// dialog never registered in Modal.svelte's topmost-open-stack, so the
// underlying modal's own Escape handler believed itself topmost and closed
// too. This test mounts an "underlying" Modal layer first (mirroring
// ManageInstancesModal, which is rendered before <InstanceIconDialog> in
// +page.svelte), opens the icon dialog on top of it, and asserts a single
// Escape keypress closes only the topmost (icon) dialog.

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
    previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
    setInstanceIcon: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    clearInstanceIcon: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
}));
vi.mock('$lib/ipc/format-error', () => ({ formatError: (e: unknown) => `fmt:${String(e)}` }));
vi.mock('$lib/instances/instance-icon-cache', () => ({
  invalidateInstanceIcon: vi.fn(),
}));
// get(t)/$t must yield an identity translate fn so template + error strings
// render without throwing.
vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: (k: string, p?: unknown) => string) => void) => {
      run((k: string) => k);
      return () => {};
    },
  },
}));

import InstanceIconDialog from '$lib/instances/InstanceIconDialog.svelte';
import { iconDialog } from '$lib/instances/instance-icon-dialog.svelte';
import Modal from '$lib/ui/Modal.svelte';

const underlyingBody = createRawSnippet(() => ({
  render: () => '<div><h2 id="underlying-title">Manage</h2><button>OK</button></div>',
}));

describe('InstanceIconDialog Modal-stack participation', () => {
  beforeEach(() => {
    iconDialog.close();
  });

  afterEach(() => {
    iconDialog.close();
  });

  it('a single Escape closes only the topmost (icon) dialog, leaving the underlying modal open', async () => {
    const onCloseUnderlying = vi.fn();

    // Mount the underlying modal first — mirrors ManageInstancesModal being
    // rendered before <InstanceIconDialog> at the page root, so DOM order
    // (== mount order) matches production.
    render(Modal, {
      props: {
        onClose: onCloseUnderlying,
        ariaLabelledby: 'underlying-title',
        children: underlyingBody,
      },
    });

    // Open the crop dialog on top — in production this happens after a picked
    // file decodes (iconDialog.pick -> onFile -> show); show() is the seam.
    iconDialog.show('inst-1');
    render(InstanceIconDialog);

    expect(iconDialog.open).toBe(true);

    await fireEvent.keyDown(window, { key: 'Escape' });

    // Topmost (icon) dialog closed...
    expect(iconDialog.open).toBe(false);
    // ...but the underlying modal's onClose was NOT invoked by the same
    // keypress. On the old hand-rolled dialog (its own <svelte:window
    // onkeydown> outside Modal's open-stack), this assertion fails: the
    // underlying modal's Escape handler also fires because it believes
    // itself topmost.
    expect(onCloseUnderlying).not.toHaveBeenCalled();
  });
});
