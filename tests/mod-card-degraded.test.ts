import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import ModCard from '$lib/mods/ModCard.svelte';

const manual = {
  filename: 'mystery.jar', sha1: 'def', source: null, project_id: null, version_id: null,
  name: 'mystery.jar', version_number: null, installed_at: '2026-01-01T00:00:00Z',
  enabled: false, enrich_attempted: false,
};

describe('ModCard degraded (summary: null)', () => {
  it('renders a manual mod with its filename and an Enable button', () => {
    render(ModCard, {
      props: {
        summary: null, installed: manual, layout: 'list',
        onInstall: () => {}, onOpenDetail: () => {}, onToggle: () => {}, onUninstall: () => {},
      },
    });
    expect(screen.getByText('mystery.jar')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Enable' })).toBeTruthy();
  });

  it('shows the pack chip and "from modpack" when packChip is set', () => {
    render(ModCard, {
      props: {
        summary: null, installed: { ...manual, source: null }, layout: 'list', packChip: 'My Pack',
        onInstall: () => {}, onOpenDetail: () => {}, onToggle: () => {}, onUninstall: () => {},
      },
    });
    expect(screen.getByText(/My Pack/)).toBeTruthy();
  });
});
