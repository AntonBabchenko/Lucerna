import { describe, expect, it } from 'vitest';
import { formatSize } from '$lib/format/size';
import { enTr } from '../test-utils/en-translator';

describe('formatSize', () => {
  it('returns empty string for null, 0, or negative', () => {
    expect(formatSize(enTr, null)).toBe('');
    expect(formatSize(enTr, 0)).toBe('');
    expect(formatSize(enTr, -5)).toBe('');
  });
  it('formats bytes under 1 KB as B', () => {
    expect(formatSize(enTr, 512)).toBe('512 B');
  });
  it('formats KB with one decimal', () => {
    expect(formatSize(enTr, 1536)).toBe('1.5 KB');
  });
  it('formats MB with one decimal', () => {
    expect(formatSize(enTr, 261361205)).toBe('249.3 MB');
  });
  it('formats the byte below the KB→MB threshold as KB', () => {
    expect(formatSize(enTr, 1048575)).toBe('1024.0 KB');
  });
  it('formats the KB→MB threshold as MB', () => {
    expect(formatSize(enTr, 1048576)).toBe('1.0 MB');
  });
});
