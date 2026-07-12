import { describe, expect, it } from 'vitest';
import { SavedForm } from '$lib/servers/settings/saved-form.svelte';

describe('SavedForm', () => {
  it('starts not-saved', () => {
    const f = new SavedForm();
    expect(f.saved).toBe(false);
  });

  it('markSaved records the signature and reports saved while it matches', () => {
    const f = new SavedForm();
    f.markSaved('{"name":"a"}');
    expect(f.saved).toBe(true);
    f.sync('{"name":"a"}');
    expect(f.saved).toBe(true);
  });

  it('sync clears saved when the signature diverges', () => {
    const f = new SavedForm();
    f.markSaved('{"name":"a"}');
    f.sync('{"name":"b"}');
    expect(f.saved).toBe(false);
  });

  it('a second markSaved re-arms after divergence', () => {
    const f = new SavedForm();
    f.markSaved('{"n":1}');
    f.sync('{"n":2}');
    expect(f.saved).toBe(false);
    f.markSaved('{"n":2}');
    expect(f.saved).toBe(true);
  });
});
