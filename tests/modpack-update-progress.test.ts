import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import ModpackUpdateProgress from '$lib/modpacks/ModpackUpdateProgress.svelte';

describe('ModpackUpdateProgress', () => {
  it('shows the preparing label and no bar when progress is null', () => {
    const { getByTestId, queryByTestId } = render(ModpackUpdateProgress, {
      props: { progress: null },
    });
    // Default locale in tests is English.
    expect(getByTestId('imported-detail-updating').textContent).toContain('Updating');
    expect(queryByTestId('imported-detail-update-bar')).toBeNull();
  });

  it('shows the file counter + name and a 25% bar at 3/12', () => {
    const { getByTestId } = render(ModpackUpdateProgress, {
      props: { progress: { current: 3, total: 12, fileName: 'Sodium' } },
    });
    const label = getByTestId('imported-detail-updating');
    expect(label.textContent).toContain('3');
    expect(label.textContent).toContain('12');
    expect(label.textContent).toContain('Sodium');
    expect(getByTestId('imported-detail-update-bar').getAttribute('style')).toContain('width: 25%');
  });

  it('names the world being backed up', () => {
    const { getByTestId } = render(ModpackUpdateProgress, {
      props: {
        progress: { current: 1, total: 2, fileName: 'Survival', phase: 'backing_up_world' },
      },
    });
    const label = getByTestId('imported-detail-updating').textContent ?? '';
    expect(label).toContain('Backing up world');
    expect(label).toContain('Survival');
  });

  it('fills the bar with the worlds already backed up, not the one being zipped', () => {
    // The only world of a profile used to show a full bar the whole time it was zipped.
    const only = render(ModpackUpdateProgress, {
      props: {
        progress: { current: 1, total: 1, fileName: 'Survival', phase: 'backing_up_world' },
      },
    });
    expect(only.getByTestId('imported-detail-update-bar').getAttribute('style')).toContain(
      'width: 0%',
    );
    only.unmount();
    const second = render(ModpackUpdateProgress, {
      props: {
        progress: { current: 2, total: 2, fileName: 'Creative', phase: 'backing_up_world' },
      },
    });
    expect(second.getByTestId('imported-detail-update-bar').getAttribute('style')).toContain(
      'width: 50%',
    );
  });

  it('says the changes are being applied, with no bar, once the files move in', () => {
    const { getByTestId, queryByTestId } = render(ModpackUpdateProgress, {
      props: { progress: { phase: 'applying_changes' } },
    });
    const label = getByTestId('imported-detail-updating').textContent ?? '';
    expect(label).toContain('Applying the changes');
    expect(label).not.toContain('Backing up');
    expect(queryByTestId('imported-detail-update-bar')).toBeNull();
  });
});
