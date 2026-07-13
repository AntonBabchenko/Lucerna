import { describe, expect, it } from 'vitest';
import {
  clampPanelWidth,
  companionCell,
  MAX_CELL,
  MIN_CELL,
  PANEL_MAX_WIDTH,
  PANEL_MIN_WIDTH,
} from '$lib/accounts/skin-editor/panel-resize';

describe('clampPanelWidth', () => {
  it('passes through a value inside the range', () => {
    expect(clampPanelWidth(300)).toBe(300);
  });

  it('clamps below the minimum', () => {
    expect(clampPanelWidth(PANEL_MIN_WIDTH - 50)).toBe(PANEL_MIN_WIDTH);
  });

  it('clamps above the maximum', () => {
    expect(clampPanelWidth(PANEL_MAX_WIDTH + 50)).toBe(PANEL_MAX_WIDTH);
  });

  it('honours custom bounds', () => {
    expect(clampPanelWidth(10, 20, 40)).toBe(20);
    expect(clampPanelWidth(100, 20, 40)).toBe(40);
  });
});

describe('companionCell', () => {
  it('floors box width / 64 into an integer texel size', () => {
    expect(companionCell(64 * 5)).toBe(5);
    expect(companionCell(400)).toBe(6); // floor(400/64) = 6
  });

  it('clamps to MIN_CELL and MAX_CELL', () => {
    expect(companionCell(10)).toBe(MIN_CELL);
    expect(companionCell(64 * 100)).toBe(MAX_CELL);
  });
});
