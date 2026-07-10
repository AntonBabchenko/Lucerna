import { beforeEach, describe, expect, it } from 'vitest';
import { loadMode, loadSelectedServer, serversUi } from '$lib/servers/servers-ui.svelte';

describe('servers-ui state', () => {
  beforeEach(() => {
    serversUi.setMode('client');
    serversUi.selectServer(null);
    serversUi.activeTab = 'console';
    serversUi.creating = false;
    localStorage.clear();
  });

  it('defaults to client mode when nothing is persisted', () => {
    expect(loadMode()).toBe('client');
  });

  it('treats any non-servers persisted value as client', () => {
    localStorage.setItem('lucerna.ui.mode', 'garbage');
    expect(loadMode()).toBe('client');
  });

  it('reads a persisted servers mode', () => {
    localStorage.setItem('lucerna.ui.mode', 'servers');
    expect(loadMode()).toBe('servers');
  });

  it('setMode flips state and persists', () => {
    serversUi.setMode('servers');
    expect(serversUi.mode).toBe('servers');
    expect(localStorage.getItem('lucerna.ui.mode')).toBe('servers');
  });

  it('selectServer persists the id and null removes it', () => {
    serversUi.selectServer('srv-1');
    expect(localStorage.getItem('lucerna.ui.selectedServer')).toBe('srv-1');
    expect(loadSelectedServer()).toBe('srv-1');
    serversUi.selectServer(null);
    expect(localStorage.getItem('lucerna.ui.selectedServer')).toBeNull();
    expect(loadSelectedServer()).toBeNull();
  });

  it('reconcile keeps a valid selection', () => {
    serversUi.selectServer('srv-1');
    serversUi.reconcile(['srv-1', 'srv-2']);
    expect(serversUi.selectedServerId).toBe('srv-1');
  });

  it('reconcile falls back to the first server for a stale id', () => {
    serversUi.selectServer('deleted');
    serversUi.reconcile(['srv-2', 'srv-3']);
    expect(serversUi.selectedServerId).toBe('srv-2');
  });

  it('reconcile auto-selects the first server when nothing selected', () => {
    serversUi.reconcile(['srv-9']);
    expect(serversUi.selectedServerId).toBe('srv-9');
  });

  it('reconcile goes to null when the list is empty', () => {
    serversUi.selectServer('gone');
    serversUi.reconcile([]);
    expect(serversUi.selectedServerId).toBeNull();
  });

  it('reconcile does not write when null selection meets an empty list', () => {
    serversUi.reconcile([]);
    expect(localStorage.getItem('lucerna.ui.selectedServer')).toBeNull();
    expect(serversUi.selectedServerId).toBeNull();
  });
});
