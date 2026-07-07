import { save } from '@tauri-apps/plugin-dialog';
import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { commands, type Screenshot } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';

// Copy a screenshot image to the OS clipboard. Returns true on success.
export async function copyToClipboard(s: Screenshot): Promise<boolean> {
  const r = await commands.copyScreenshotToClipboard(s.instance_id, s.file_name);
  if (r.status === 'ok') {
    pushSuccess(get(t)('screenshots.toastCopied'));
    return true;
  }
  pushWarning(formatError(r.error));
  return false;
}

// Prompt for a destination and save a copy of the original file.
export async function saveCopy(s: Screenshot): Promise<boolean> {
  const dest = await save({
    title: get(t)('screenshots.saveDialogTitle'),
    defaultPath: s.file_name,
  });
  if (!dest) return false;
  const r = await commands.saveScreenshotCopy(s.instance_id, s.file_name, dest);
  if (r.status === 'ok') {
    pushSuccess(get(t)('screenshots.toastSaved'));
    return true;
  }
  pushWarning(formatError(r.error));
  return false;
}

// Prompt for a destination and save a copy with the annotations (and crop)
// baked in — composited onto the original at full resolution by the backend.
// `overlayDataUrl` is the strokes' transparent PNG (or null), `crop` is the
// applied crop as 0..1 fractions (or null).
export async function saveAnnotated(
  s: Screenshot,
  overlayDataUrl: string | null,
  crop: { x: number; y: number; w: number; h: number } | null,
): Promise<boolean> {
  const dest = await save({
    title: get(t)('screenshots.saveDialogTitle'),
    defaultPath: s.file_name,
  });
  if (!dest) return false;
  const overlay = overlayDataUrl ? overlayDataUrl.replace(/^data:image\/png;base64,/, '') : '';
  const r = await commands.saveAnnotatedScreenshot(s.instance_id, s.file_name, overlay, crop, dest);
  if (r.status === 'ok') {
    pushSuccess(get(t)('screenshots.toastSaved'));
    return true;
  }
  pushWarning(formatError(r.error));
  return false;
}

// Reveal the file in the OS file manager.
export async function reveal(s: Screenshot): Promise<void> {
  const r = await commands.revealScreenshot(s.instance_id, s.file_name);
  if (r.status === 'error') pushWarning(formatError(r.error));
}

// Move the file to the recycle bin. Returns true on success.
export async function deleteScreenshot(s: Screenshot): Promise<boolean> {
  const r = await commands.deleteScreenshot(s.instance_id, s.file_name);
  if (r.status === 'ok') {
    pushSuccess(get(t)('screenshots.toastDeleted'));
    return true;
  }
  pushWarning(formatError(r.error));
  return false;
}
