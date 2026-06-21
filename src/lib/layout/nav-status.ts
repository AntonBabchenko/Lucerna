// The single source for how a sidebar nav button signals state: which colour /
// pulse the leading icon gets, and whether the trailing fix wrench shows. Used
// by both the "Servers" and "Logs" buttons so the rule lives once (see
// docs/DESIGN.md §14). Labels stay in the buttons because they are
// button-specific i18n strings.

export type NavStatusKind =
  | 'idle'
  | 'running' // servers: a server is up
  | 'crashed' // servers: a server exited non-zero with no one-click fix
  | 'fixable' // servers: a crash with a one-click fix available
  | 'advisory' // logs: a non-actionable notice
  | 'actionable'; // logs: a one-click fix available

export interface NavVisual {
  /** Class applied to the leading <Icon> (colour / pulse). '' = neutral. */
  iconClass: string;
  /** Whether to render the trailing fix wrench after the label. */
  wrench: boolean;
}

export function navVisual(kind: NavStatusKind): NavVisual {
  switch (kind) {
    case 'running':
      return { iconClass: 'nav-icon-running', wrench: false };
    case 'crashed':
      return { iconClass: 'text-danger', wrench: false };
    case 'fixable':
    case 'actionable':
      return { iconClass: 'text-warning-text', wrench: true };
    // advisory shares the amber colour with fixable/actionable; wrench:false is the distinction.
    case 'advisory':
      return { iconClass: 'text-warning-text', wrench: false };
    case 'idle':
      return { iconClass: '', wrench: false };
  }
}
