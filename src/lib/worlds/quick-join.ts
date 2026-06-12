const MAX_ADDRESS_LEN = 260;

/**
 * Client-side mirror of the Rust `validate_server_address` rules: non-empty,
 * no whitespace/control chars, length <= MAX_ADDRESS_LEN, optional single
 * `:port` parsing as 0-65535. IPv6 literals are out of scope for v1.
 */
export function isValidServerAddress(address: string): boolean {
  if (address.length === 0 || address.length > MAX_ADDRESS_LEN) return false;
  if (/\s/.test(address)) return false;
  // biome-ignore lint/suspicious/noControlCharactersInRegex: server-address sanitization
  if (/[\x00-\x1f\x7f]/.test(address)) return false;
  const colon = address.indexOf(':');
  if (colon === -1) return true;
  if (colon !== address.lastIndexOf(':')) return false; // multiple colons
  const host = address.slice(0, colon);
  const port = address.slice(colon + 1);
  if (host.length === 0 || port.length === 0) return false;
  if (!/^\d+$/.test(port)) return false;
  const n = Number(port);
  return Number.isInteger(n) && n >= 0 && n <= 65535;
}
