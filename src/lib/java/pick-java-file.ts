// The native file dialog for picking a Java executable — the Java field's
// Browse and the rescue's. The filter is a convenience (filters take extensions
// only); the backend refuses a file that is not a Java either way.
import { open as openFile } from '@tauri-apps/plugin-dialog';
import { get } from 'svelte/store';
import { t } from '$lib/i18n';

function onWindows(): boolean {
  return typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);
}

/** The picked path, or `null` when the dialog was cancelled. Throws when the
 *  dialog itself failed, so the caller can say so. */
export async function pickJavaFile(): Promise<string | null> {
  const picked = await openFile({
    multiple: false,
    directory: false,
    filters: onWindows()
      ? [{ name: get(t)('instance.manage.java.fileFilter'), extensions: ['exe'] }]
      : undefined,
  });
  return typeof picked === 'string' ? picked : null;
}
