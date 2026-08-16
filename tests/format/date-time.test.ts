import { describe, expect, it } from 'vitest';
import { formatDate, formatDateTime, formatTimeOfDay } from '$lib/format/date-time';

// Assertions pick locale-DISTINCTIVE content rather than a full pinned string:
// the exact pattern Intl produces for a tag moves with the ICU data bundled in
// the Node build, and pinning it would fail on a version bump instead of on a
// real regression. Same approach as the groupLabel cases in
// tests/screenshots-view.test.ts, which assert /январ/i rather than a date.
//
// The timestamp is built from LOCAL components on purpose, so the rendered
// wall-clock time is 14:30 in every CI timezone.
const JAN_2026 = new Date(2026, 0, 5, 14, 30).getTime();

describe('formatDate', () => {
  it('renders the month name in the locale it is passed', () => {
    expect(formatDate('en', JAN_2026, { month: 'long' })).toMatch(/january/i);
    expect(formatDate('ru', JAN_2026, { month: 'long' })).toMatch(/январ/i);
  });

  it('orders the numeric date the way the locale does', () => {
    // en-US puts the month first; ru-RU the day, zero-padded.
    expect(formatDate('en', JAN_2026)).toMatch(/^1\D+5\D+2026/);
    expect(formatDate('ru', JAN_2026)).toMatch(/^05\D+01\D+2026/);
  });

  it('falls back to the host default before the locale store resolves', () => {
    // null is what svelte-i18n yields until initLocale() reconciles the
    // persisted preference — it must not throw or render "Invalid Date".
    expect(formatDate(null, JAN_2026)).not.toContain('Invalid');
    expect(formatDate(undefined, JAN_2026)).not.toContain('Invalid');
  });
});

describe('formatDateTime', () => {
  it('includes both the calendar date and the time of day', () => {
    const en = formatDateTime('en', JAN_2026);
    expect(en).toMatch(/2026/);
    expect(en).toMatch(/2:30|14:30/);
  });

  it('honours the locale it is passed rather than the OS locale', () => {
    expect(formatDateTime('ru', JAN_2026)).toMatch(/05\.01\.2026/);
  });
});

describe('formatTimeOfDay', () => {
  it('drops the date and uses the locale clock', () => {
    const ru = formatTimeOfDay('ru', JAN_2026);
    expect(ru).toMatch(/14:30/);
    expect(ru).not.toMatch(/2026/);
  });

  it('forwards explicit options (the JournalRow hour/minute shape)', () => {
    expect(formatTimeOfDay('en', JAN_2026, { hour: '2-digit', minute: '2-digit' })).toMatch(
      /02:30|14:30/,
    );
  });
});
