import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import SplitterHandle from '$lib/ui/SplitterHandle.svelte';

// The handle carries no label, no icon and no hover affordance until the
// pointer is already on it, so Tab is the only way a keyboard user finds it —
// which makes the global focus ring the one cue that says "you are here".
// happy-dom has no layout, so the drawn outline is not observable; the class
// that used to suppress it is.
describe('SplitterHandle', () => {
  const props = { width: 280, min: 220, max: 420, label: 'Resize the list', testId: 'splitter' };

  it('does not suppress the global focus ring', () => {
    render(SplitterHandle, { props });
    const el = screen.getByTestId('splitter');
    expect(el.className.split(/\s+/)).not.toContain('focus:outline-none');
  });
});
