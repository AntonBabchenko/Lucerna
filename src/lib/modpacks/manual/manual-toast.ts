import { get } from 'svelte/store';
import type { ReportLine } from '$lib/format/reason-lines';
import { t } from '$lib/i18n';
import { pushActionToast } from '$lib/toasts/toasts.svelte';
import { openManualDownloads } from './manual-downloads.svelte';

/** The completion toast when a pack import or update left files to download by hand (§5.4 item 1). */
export function pushManualDownloadsToast(
  kind: 'imported' | 'updated',
  name: string,
  instanceId: string,
  count: number,
  lines: ReportLine[] = [],
  openInstance?: () => void,
): void {
  const tr = get(t);
  pushActionToast(
    'warning',
    tr(kind === 'imported' ? 'modpacks.manual.toast.imported' : 'modpacks.manual.toast.updated', {
      name,
      count,
    }),
    { label: tr('modpacks.manual.toast.finish'), run: () => openManualDownloads(instanceId) },
    lines,
    openInstance ? { secondary: { label: tr('ops.openInstance'), run: openInstance } } : {},
  );
}
