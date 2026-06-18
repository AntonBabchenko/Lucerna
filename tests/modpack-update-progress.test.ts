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
});
