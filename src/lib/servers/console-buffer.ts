/** Maximum number of console lines retained per server before oldest are dropped. */
export const MAX_CONSOLE_LINES = 500;

/**
 * Append `line` to `arr`, keeping at most `max` entries by dropping the
 * oldest. Always returns a new array (immutable update).
 */
export function appendCapped(arr: string[], line: string, max: number): string[] {
	const next = [...arr, line];
	return next.length > max ? next.slice(next.length - max) : next;
}
