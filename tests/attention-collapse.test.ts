import { beforeEach, describe, expect, it, vi } from 'vitest';
import { attentionCollapse } from '$lib/overview/attention-collapse.svelte';

beforeEach(() => {
  localStorage.clear();
  attentionCollapse.reset();
});

describe('attentionCollapse', () => {
  it('defaults to not collapsed', () => {
    expect(attentionCollapse.isCollapsed('i1')).toBe(false);
  });

  it('collapses and expands a single instance', () => {
    attentionCollapse.setCollapsed('i1', true);
    expect(attentionCollapse.isCollapsed('i1')).toBe(true);
    attentionCollapse.setCollapsed('i1', false);
    expect(attentionCollapse.isCollapsed('i1')).toBe(false);
  });

  it('keeps collapse state independent per instance', () => {
    attentionCollapse.setCollapsed('i1', true);
    expect(attentionCollapse.isCollapsed('i1')).toBe(true);
    expect(attentionCollapse.isCollapsed('i2')).toBe(false);
  });

  it('persists the collapsed set to localStorage', () => {
    attentionCollapse.setCollapsed('i1', true);
    const raw = localStorage.getItem('lucerna.attentionCollapsed');
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw as string)).toContain('i1');
  });

  it('drops an instance from localStorage when expanded again', () => {
    attentionCollapse.setCollapsed('i1', true);
    attentionCollapse.setCollapsed('i1', false);
    const raw = localStorage.getItem('lucerna.attentionCollapsed');
    expect(JSON.parse(raw as string)).not.toContain('i1');
  });

  it('reloads collapsed state from localStorage on a fresh import', async () => {
    localStorage.setItem('lucerna.attentionCollapsed', JSON.stringify(['i7']));
    vi.resetModules();
    const fresh = await import('$lib/overview/attention-collapse.svelte');
    expect(fresh.attentionCollapse.isCollapsed('i7')).toBe(true);
    expect(fresh.attentionCollapse.isCollapsed('i1')).toBe(false);
  });
});
