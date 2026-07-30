/**
 * Which control the Manage modal should call out when it opens.
 *
 * Set by an Overview card the user clicked ("I was looking at Memory"), so the
 * modal can scroll to that control and flash it instead of dumping the user at
 * the top of a seven-field form. `null` means "no particular field" — the
 * modal opens exactly as it always did.
 */
export type ManageFocusField = 'mc' | 'loader' | 'memory' | 'integrity';

/**
 * Fields where moving keyboard focus is safe. The memory slider is excluded on
 * purpose: focusing a range input means a stray arrow key or scroll wheel
 * silently rewrites the instance's heap size. `integrity` is a section, not a
 * control, so there is nothing to focus.
 */
const FOCUSABLE_FIELDS: readonly ManageFocusField[] = ['mc', 'loader'];

export function shouldFocusField(field: ManageFocusField): boolean {
  return FOCUSABLE_FIELDS.includes(field);
}
