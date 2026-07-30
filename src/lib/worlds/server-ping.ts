import type { ServerPingOutcome } from '$lib/ipc/bindings';
import { mapLimit } from '$lib/mods/concurrency';

/**
 * A row's status: absent (never asked), `'pending'` (ping in flight), or the
 * outcome the backend returned.
 */
export type PingState = 'pending' | ServerPingOutcome;

/**
 * Frontend pool size. The real ceiling is the semaphore in Rust
 * (`network::consent`) — this only keeps the backend queue short so rows fill
 * in steadily instead of all waiting behind one another.
 */
export const PING_POOL = 4;

/**
 * Render the chip text from whatever the server actually reported.
 *
 * Every SLP field is optional on the wire, so a missing part is left out rather
 * than rendered as `?` or `0` — a placeholder reads as data the server sent.
 */
export function formatPingChip(outcome: Extract<ServerPingOutcome, { kind: 'online' }>): string {
  const parts: string[] = [];
  if (outcome.players_online !== null && outcome.players_max !== null) {
    parts.push(`${outcome.players_online}/${outcome.players_max}`);
  } else if (outcome.players_online !== null) {
    parts.push(String(outcome.players_online));
  }
  if (outcome.version_name) parts.push(outcome.version_name);
  parts.push(`${outcome.latency_ms} ms`);
  return parts.join(' · ');
}

/**
 * Ping every distinct address with a bounded pool, handing each result to
 * `onResult` the moment it lands so rows update progressively rather than all
 * at the end. Duplicates in the list are pinged once.
 */
export async function sweepPings(
  addresses: readonly string[],
  ping: (address: string) => Promise<ServerPingOutcome | null>,
  onResult: (address: string, outcome: ServerPingOutcome | null) => void,
): Promise<void> {
  const distinct = [...new Set(addresses)];
  await mapLimit(distinct, PING_POOL, async (address) => {
    onResult(address, await ping(address));
  });
}
