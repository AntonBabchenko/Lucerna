import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import { displayLoader } from '$lib/instances/loader-display';
import type { Error as IpcError, LoaderKind } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';

/**
 * Why a loader's version list did not load, shared by the instance loader
 * picker and the server core picker.
 *
 * A network failure gets a headline naming WHAT did not load, then the
 * failure's own sentence (which names the site and the reason) — the
 * `withCause` shape. The two refusals the pickers meet most keep their short
 * wording; everything else delegates to `formatError`, so no variant can reach
 * the picker as raw JSON or as its English `Display`.
 */
export function formatLoaderListError(e: IpcError, loader: LoaderKind): string {
  const translate = get(t);
  if (e.kind === 'network' || e.kind === 'mods_network')
    return `${translate('instance.loader.errorVersionList', { loader: displayLoader(loader) })} ${formatError(e)}`;
  if (e.kind === 'loader_unavailable')
    return translate('instance.loader.errorUnavailable', {
      loader: displayLoader(e.loader as LoaderKind),
      mc: e.mc_version,
    });
  if (e.kind === 'forge_promotions_unavailable')
    return translate('instance.loader.errorForgePromotions', { flavor: e.flavor });
  return formatError(e);
}
