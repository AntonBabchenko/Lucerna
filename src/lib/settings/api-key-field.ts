// Types shared by `ApiKeyField.svelte` and the two forms that drive it.

/** DESIGN.md "Inline status words": one token family per tone, on text. */
export type StatusTone = 'success' | 'secondary' | 'warning' | 'danger' | 'placeholder';
export type ResultTone = 'danger' | 'warning' | 'info';

/** Fact A — what is stored. `busy` = the read is in flight (one Spinner, one live region). */
export type FieldStatus = { text: string; tone: StatusTone; busy?: boolean };
/** Fact B — what happened to what was just typed; rendered through `StatusMessage`. */
export type FieldResult = { tone: ResultTone; text: string };

export const STATUS_TONE_CLASS: Record<StatusTone, string> = {
  success: 'text-success font-medium',
  secondary: 'text-secondary',
  warning: 'text-warning-text font-medium',
  danger: 'text-danger font-medium',
  placeholder: 'text-placeholder',
};
