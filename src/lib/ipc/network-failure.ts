import { get } from 'svelte/store';
import { t } from '$lib/i18n';
import type { NetworkFailure } from '$lib/ipc/bindings';

/**
 * The host of a request URL: what the user can recognise and add to an
 * antivirus or VPN exception, and unambiguous where "the server" would read as
 * their own Minecraft server. Never the full URL (the transport policy keeps
 * paths and queries out of the UI). The backend only sends absolute URLs; a
 * string that does not parse is shown as it is rather than hidden.
 */
export function hostOf(url: string): string {
  try {
    return new URL(url).hostname || url;
  } catch {
    return url;
  }
}

/** A server on this computer — in practice the user's local model server, the
 *  only loopback destination the backend reaches (`network::loopback`). Not
 *  answering there is a program that is not running, not the internet. */
function isLoopback(host: string): boolean {
  return host === '127.0.0.1' || host === 'localhost' || host === '[::1]' || host === '::1';
}

function httpStatusMessage(host: string, status: number): string {
  const translate = get(t);
  if (status === 429) return translate('errors.networkFailure.rateLimited', { host });
  if (status === 404) return translate('errors.networkFailure.notFound', { host });
  // `status` as a string: an ICU number argument would be locale-formatted.
  const code = String(status);
  if (status >= 500 && status <= 599)
    return translate('errors.networkFailure.serverError', { host, status: code });
  if (status >= 400 && status <= 499)
    return translate('errors.networkFailure.refused', { host, status: code });
  return translate('errors.networkFailure.unexpectedStatus', { host, status: code });
}

/**
 * One sentence per failure, naming the site. Only `other` points at Logs: a
 * recognised cause is a complete sentence, and `other` is produced only by a
 * backend path that wrote the cause chain to the log
 * (`structural_network_failure_other.rs`).
 */
export function networkFailureMessage(url: string, failure: NetworkFailure): string {
  const translate = get(t);
  const host = hostOf(url);
  switch (failure.kind) {
    case 'unreachable':
      return isLoopback(host)
        ? translate('errors.networkFailure.unreachableLocal', { host })
        : translate('errors.networkFailure.unreachable', { host });
    case 'timeout':
      return translate('errors.networkFailure.timeout', { host });
    case 'tls':
      return translate('errors.networkFailure.tls', { host });
    case 'interrupted':
      return translate('errors.networkFailure.interrupted', { host });
    case 'http_status':
      return httpStatusMessage(host, failure.status);
    case 'bad_response':
      return translate('errors.networkFailure.badResponse', { host });
    case 'other':
      return translate('errors.networkFailure.other', { host });
    default: {
      const _exhaustive: never = failure;
      return _exhaustive;
    }
  }
}
