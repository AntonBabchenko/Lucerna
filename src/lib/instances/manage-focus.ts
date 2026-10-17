/**
 * Which control the instance Settings tab should call out when it opens.
 *
 * Set by whatever sent the user there: an Overview card ("I was looking at
 * Memory"), an attention item, or the row menu's Rename. The tab scrolls to that
 * control and flashes it instead of dumping the user at the top of the form.
 * `null` means "no particular field".
 */
export type ManageFocusField = 'mc' | 'loader' | 'memory' | 'integrity' | 'name' | 'java';

/**
 * Fields where moving keyboard focus is safe. The memory slider is excluded on
 * purpose: focusing a range input means a stray arrow key or scroll wheel
 * silently rewrites the instance's heap size. `integrity` is a section, not a
 * control, so there is nothing to focus. `name` is focused AND selected — it is
 * where Rename lands. `java` is the Java list, where «Choose another Java» lands.
 */
const FOCUSABLE_FIELDS: readonly ManageFocusField[] = ['mc', 'loader', 'name', 'java'];

export function shouldFocusField(field: ManageFocusField): boolean {
  return FOCUSABLE_FIELDS.includes(field);
}
