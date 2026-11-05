// Which instance the Finish-install dialog is open for (spec 2026-11-05 §5.1). Asked from four
// places — the completion toasts, the imported-pack drawer, the Overview row, the Play gate — so
// it is a module store and `ManualDownloadsHost` renders it (DESIGN §8, app-level dialogs).
export const manualDownloads = $state<{ instanceId: string | null; changed: number }>({
  instanceId: null,
  changed: 0,
});

export function openManualDownloads(instanceId: string): void {
  manualDownloads.instanceId = instanceId;
}

export function closeManualDownloads(): void {
  manualDownloads.instanceId = null;
}

/** A file was placed: views that show the pack's files read again (assets emit no event). */
export function noteManualDownloadsChanged(): void {
  manualDownloads.changed += 1;
}
