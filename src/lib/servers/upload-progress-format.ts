import { formatSize } from '$lib/format/size';
import type { Translate } from '$lib/i18n';

/**
 * Pure helpers for the Hosting-tab upload progress line (#G). No Svelte, no IPC:
 * the component samples the store's byte stream, this module turns those samples
 * into a smoothed speed (EWMA), an ETA, and a localized one-line string. Mirrors
 * the `(t: Translate, …) => string` shape of `$lib/format/duration.ts`.
 */

/** A single observation of cumulative bytes uploaded at a wall-clock time. */
export interface SpeedSample {
  bytes: number;
  atMs: number;
}

/** EWMA smoothing factor: weight of the newest interval (0..1). Lower = smoother. */
const EWMA_ALPHA = 0.3;

const KB = 1024;
const MB = 1024 * 1024;

/**
 * Exponentially-weighted moving average of the transfer rate (bytes/sec) over a
 * series of cumulative-byte samples. Intervals with a non-positive time delta are
 * skipped (clock jitter / duplicate samples). Fewer than two usable samples → 0.
 */
export function estimateSpeedBytesPerSec(samples: readonly SpeedSample[]): number {
  let ewma: number | null = null;
  for (let i = 1; i < samples.length; i++) {
    const dt = samples[i].atMs - samples[i - 1].atMs;
    if (dt <= 0) continue;
    const db = samples[i].bytes - samples[i - 1].bytes;
    const rate = (db / dt) * 1000;
    ewma = ewma === null ? rate : EWMA_ALPHA * rate + (1 - EWMA_ALPHA) * ewma;
  }
  return ewma === null ? 0 : Math.max(0, ewma);
}

/**
 * Seconds remaining = remaining_bytes / speed. Returns null when speed is
 * non-positive (cannot estimate), 0 when already complete (clamped).
 */
export function etaSeconds(
  bytesTotal: number,
  bytesDone: number,
  speedBytesPerSec: number,
): number | null {
  if (speedBytesPerSec <= 0) return null;
  const remaining = Math.max(0, bytesTotal - bytesDone);
  return remaining / speedBytesPerSec;
}

/**
 * Localized transfer rate, e.g. "1.5 MB/s" / "1,5 МБ/с". Picks the unit by
 * magnitude and hands `t()` the RAW number: the dictionary's
 * `{n, number, ::.0 group-off}` argument owns the rounding AND the decimal
 * separator, exactly as `formatSize` does. A `.toFixed(1)` string carries the
 * ENGLISH dot into every locale, and `formatUploadProgress` puts this next to
 * formatSize's output — so one row read "1,5 МБ / 4,0 МБ · 1.5 МБ/с".
 */
export function formatRate(t: Translate, bytesPerSec: number): string {
  const v = Math.max(0, bytesPerSec);
  if (v < KB) return t('format.rate.bytesPerSec', { n: Math.round(v) });
  if (v < MB) return t('format.rate.kbPerSec', { n: v / KB });
  return t('format.rate.mbPerSec', { n: v / MB });
}

/** "M:SS" clock for the ETA; "--:--" when unknown (null). */
export function formatEtaClock(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds)) return '--:--';
  const total = Math.max(0, Math.round(seconds));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, '0')}`;
}

export interface UploadProgressView {
  bytesDone: number;
  bytesTotal: number;
  speedBytesPerSec: number;
  etaSecondsValue: number | null;
}

/**
 * One-line progress string: "X.X MB / Y.Y MB · Z.Z MB/s · ~M:SS". The byte
 * sizes reuse `formatSize` so they localize; the ETA renders as "~--:--" while
 * speed is still unknown.
 */
export function formatUploadProgress(t: Translate, v: UploadProgressView): string {
  return t('servers.hosting.uploadingBytes', {
    doneSize: formatSize(t, v.bytesDone) || t('format.size.bytes', { n: 0 }),
    totalSize: formatSize(t, v.bytesTotal) || t('format.size.bytes', { n: 0 }),
    rate: formatRate(t, v.speedBytesPerSec),
    eta: formatEtaClock(v.etaSecondsValue),
  });
}

/** Cap on the cumulative-byte sample buffer feeding the EWMA speed estimate. */
const MAX_SPEED_SAMPLES = 30;

/**
 * Throttled snapshot of the numbers actually shown on the progress line. The
 * `samples`/`lastRefreshMs` fields are internal bookkeeping for `advanceProgressDisplay`.
 */
export interface ProgressDisplay {
  bytesDone: number;
  speedBytesPerSec: number;
  etaSecondsValue: number | null;
  /** Internal: cumulative-byte samples for the EWMA speed estimate. */
  samples: SpeedSample[];
  /** Internal: wall-clock (ms) of the last refresh; 0 means "never refreshed". */
  lastRefreshMs: number;
}

/** A fresh, empty display — used before an upload starts and on reset. */
export function emptyProgressDisplay(): ProgressDisplay {
  return {
    bytesDone: 0,
    speedBytesPerSec: 0,
    etaSecondsValue: null,
    samples: [],
    lastRefreshMs: 0,
  };
}

/**
 * Advance the throttled progress display. The shown speed/ETA/bytes refresh at
 * most once per `refreshMs`: within that window this returns `prev` UNCHANGED
 * (same reference), so the rendered line is frozen and does not flicker under
 * the high-frequency progress events of a parallel upload (~100/s on
 * many-small-file sets). On a refresh it appends a calm ~1-per-`refreshMs` byte
 * sample, recomputes the EWMA speed over that smooth series, and derives the
 * ETA. The progress BAR is driven by live bytes elsewhere — it is intentionally
 * NOT throttled here, so it keeps filling smoothly.
 *
 * Pure and deterministic: `nowMs` is passed in, never read from the clock.
 */
export function advanceProgressDisplay(
  prev: ProgressDisplay,
  bytesDone: number,
  bytesTotal: number,
  nowMs: number,
  refreshMs: number,
): ProgressDisplay {
  if (prev.lastRefreshMs !== 0 && nowMs - prev.lastRefreshMs < refreshMs) {
    return prev; // inside the throttle window — freeze the display
  }
  const last = prev.samples[prev.samples.length - 1];
  const samples =
    last && last.bytes === bytesDone
      ? prev.samples // no new bytes since the last refresh — don't add a duplicate
      : [...prev.samples, { bytes: bytesDone, atMs: nowMs }].slice(-MAX_SPEED_SAMPLES);
  const speed = estimateSpeedBytesPerSec(samples);
  return {
    bytesDone,
    speedBytesPerSec: speed,
    etaSecondsValue: etaSeconds(bytesTotal, bytesDone, speed),
    samples,
    lastRefreshMs: nowMs,
  };
}
