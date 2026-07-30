import { fireEvent, render } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const pick = vi.fn();
const requestRemove = vi.fn();
vi.mock('$lib/instances/instance-icon-dialog.svelte', () => ({
  iconDialog: {
    pick: (id: string) => pick(id),
    requestRemove: (id: string) => requestRemove(id),
  },
}));

vi.mock('$lib/instances/instance-icon-cache', () => ({
  loadInstanceIcon: vi.fn().mockResolvedValue('data:image/png;base64,AAAA'),
  invalidateInstanceIcon: vi.fn(),
}));

import InstanceAvatarEdit from '$lib/instances/InstanceAvatarEdit.svelte';

const base = {
  id: 'i1',
  name: 'Skyblock',
  loader: 'fabric' as const,
  mrpack_source: null,
  has_icon: false,
};

describe('InstanceAvatarEdit', () => {
  beforeEach(() => {
    pick.mockReset();
    requestRemove.mockReset();
  });

  it('opens the picker for its instance when the avatar is clicked', async () => {
    const { getByTestId } = render(InstanceAvatarEdit, {
      props: { instance: base, testId: 'avatar' },
    });
    await fireEvent.click(getByTestId('avatar'));
    expect(pick).toHaveBeenCalledWith('i1');
  });

  it('offers no remove button when the instance has no custom picture', () => {
    const { queryByTestId } = render(InstanceAvatarEdit, {
      props: { instance: base, removeTestId: 'remove' },
    });
    expect(queryByTestId('remove')).toBeNull();
  });

  it('asks to remove the picture when the corner button is clicked', async () => {
    const { getByTestId } = render(InstanceAvatarEdit, {
      props: { instance: { ...base, has_icon: true }, removeTestId: 'remove' },
    });
    await fireEvent.click(getByTestId('remove'));
    expect(requestRemove).toHaveBeenCalledWith('i1');
  });
});
