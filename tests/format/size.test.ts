import { describe, expect, it } from 'vitest';
import { formatSize } from '$lib/format/size';

describe('formatSize', () => {
  it('returns empty string for null, 0, or negative', () => {
    expect(formatSize(null)).toBe('');
    expect(formatSize(0)).toBe('');
    expect(formatSize(-5)).toBe('');
  });
  it('formats bytes under 1 KiB as B', () => {
    expect(formatSize(512)).toBe('512 B');
  });
  it('formats KiB with one decimal', () => {
    expect(formatSize(1536)).toBe('1.5 KiB');
  });
  it('formats MiB with one decimal', () => {
    expect(formatSize(261361205)).toBe('249.3 MiB');
  });
  it('formats the byte below the KiB→MiB threshold as KiB', () => {
    expect(formatSize(1048575)).toBe('1024.0 KiB');
  });
  it('formats the KiB→MiB threshold as MiB', () => {
    expect(formatSize(1048576)).toBe('1.0 MiB');
  });
});
