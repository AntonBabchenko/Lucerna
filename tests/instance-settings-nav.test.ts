import { beforeEach, describe, expect, it, vi } from 'vitest';
import { shouldFocusField } from '$lib/instances/manage-focus';
import {
  abandonInstanceSettingsFor,
  clearInstanceSettingsNav,
  instanceSettingsNav,
  requestInstanceSettings,
  requestIsFor,
  settleInstanceSettingsNav,
} from '$lib/instances/settings/instance-settings-nav.svelte';

const setMode = vi.hoisted(() => vi.fn());
vi.mock('$lib/servers/servers-ui.svelte', () => ({ serversUi: { setMode } }));

beforeEach(() => {
  clearInstanceSettingsNav();
  setMode.mockClear();
});

describe('instance settings navigation', () => {
  it('a request switches to client mode and records the target and field', () => {
    requestInstanceSettings('b', 'name');
    expect(setMode).toHaveBeenCalledWith('client');
    expect(instanceSettingsNav.value).toEqual({ instanceId: 'b', field: 'name' });
  });

  it('a request for the active instance, or for no particular one, is deliverable', () => {
    expect(requestIsFor({ instanceId: null, field: null }, 'a')).toBe(true);
    expect(requestIsFor({ instanceId: 'a', field: null }, 'a')).toBe(true);
    expect(requestIsFor({ instanceId: 'b', field: null }, 'a')).toBe(false);
    expect(requestIsFor(null, 'a')).toBe(false);
  });

  it('a failed activation of the awaited instance drops the request', () => {
    requestInstanceSettings('b');
    settleInstanceSettingsNav('b', 'a');
    expect(instanceSettingsNav.value).toBeNull();
  });

  it('a successful activation of the awaited instance keeps it for the tab', () => {
    requestInstanceSettings('b');
    settleInstanceSettingsNav('b', 'b');
    expect(instanceSettingsNav.value).toEqual({ instanceId: 'b', field: null });
  });

  it('an activation of some other instance settling leaves the request alone', () => {
    requestInstanceSettings('b');
    settleInstanceSettingsNav('c', 'c');
    expect(instanceSettingsNav.value).not.toBeNull();
  });

  it('picking another instance abandons a request that waits on a different one', () => {
    requestInstanceSettings('b');
    abandonInstanceSettingsFor('b');
    expect(instanceSettingsNav.value).not.toBeNull();
    abandonInstanceSettingsFor('c');
    expect(instanceSettingsNav.value).toBeNull();
  });

  it('a request for whichever instance is active is never abandoned by a pick', () => {
    requestInstanceSettings(null, 'memory');
    abandonInstanceSettingsFor('c');
    expect(instanceSettingsNav.value).toEqual({ instanceId: null, field: 'memory' });
  });

  it('the name field takes focus, the memory slider still does not', () => {
    expect(shouldFocusField('name')).toBe(true);
    expect(shouldFocusField('memory')).toBe(false);
  });
});
