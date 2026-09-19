import type { IconName } from '$lib/ui/icons';

/** One row of the shared `Menu` surface (`OverflowMenu` and `ContextMenu`). */
export interface ContextMenuItem {
  label: string;
  icon?: IconName;
  danger?: boolean;
  disabled?: boolean;
  /** Why the item is disabled. Rendered as a second line under the label, so
   *  it reaches keyboard and screen-reader users without a hover. Ignored
   *  while the item is enabled. */
  disabledReason?: string;
  separatorBefore?: boolean;
  testId?: string;
  onSelect: () => void;
}
