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

  it('renders a failed-lookup platform mod with its name and a source label', () => {
    const failed = {
      filename: 'jei-15.0.jar', sha1: 'abc', source: 'modrinth' as const, project_id: 'p', version_id: 'v',
      name: 'Just Enough Items', version_number: '15.0', installed_at: '2026-01-01T00:00:00Z',
      enabled: true, enrich_attempted: false,
    };
    render(ModCard, {
      props: {
        summary: null, installed: failed, layout: 'list',
        onInstall: () => {}, onOpenDetail: () => {}, onToggle: () => {}, onUninstall: () => {},
      },
    });
    // Title uses the platform name, not the filename.
    expect(screen.getByText('Just Enough Items')).toBeTruthy();
    // Subtitle mentions the source + "details unavailable".
    expect(screen.getByText(/Modrinth/)).toBeTruthy();
    expect(screen.getByText(/details unavailable/i)).toBeTruthy();
  });
});
