/**
 * Tests for PreflightPanel: a long violation list must be wrapped in a
 * scrollable container so it cannot push the panel (or, when reused inside
 * the launch gate, the dialog's footer buttons) past the window edge.
 *
 * i18n resolves to real EN strings in the test environment.
 */
import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { DepViolation, PreflightReport } from '$lib/ipc/bindings';
import PreflightPanel from '$lib/mods/PreflightPanel.svelte';

function missing(i: number): DepViolation {
  return {
    kind: 'missing_required',
    dependent_name: `Mod ${i}`,
    dependent_sha1: `sha${i}`,
    dep_id: `dep-${i}`,
    dep_display_name: null,
    needed: '',
    installed_version: null,
    provider_project: null,
  };
}

function reportWith(count: number): PreflightReport {
  return { violations: Array.from({ length: count }, (_, i) => missing(i)) };
}

describe('PreflightPanel', () => {
  it('renders nothing when there are no violations', () => {
    const { queryByTestId } = render(PreflightPanel, {
      props: { report: { violations: [] }, onUpdate: () => {} },
    });
    expect(queryByTestId('preflight-panel')).toBeNull();
  });

  it('wraps the rows in a scrollable container', () => {
    const { getByTestId } = render(PreflightPanel, {
      props: { report: reportWith(40), onUpdate: () => {} },
    });
    const scroll = getByTestId('preflight-scroll');
    // The scroll container caps height and scrolls overflow.
    expect(scroll.className).toContain('overflow-y-auto');
    expect(scroll.className).toContain('max-h-');
    // Keyboard users must be able to focus and scroll the region.
    expect(scroll.getAttribute('tabindex')).toBe('0');
    expect(scroll.getAttribute('role')).toBe('region');
  });

  it('keeps every violation row inside the scroll container', () => {
    const { getByTestId, getAllByTestId } = render(PreflightPanel, {
      props: { report: reportWith(40), onUpdate: () => {} },
    });
    const scroll = getByTestId('preflight-scroll');
    const rows = getAllByTestId('preflight-row');
    expect(rows).toHaveLength(40);
    for (const row of rows) {
      expect(scroll.contains(row)).toBe(true);
    }
  });
});
