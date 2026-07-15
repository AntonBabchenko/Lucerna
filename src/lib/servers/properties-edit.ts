/** Read a property value from raw `key=value` text (last wins), or null. */
export function getProperty(raw: string, key: string): string | null {
  let found: string | null = null;
  for (const line of raw.split(/\r?\n/)) {
    const t = line.trimStart();
    if (t.startsWith('#') || !t.includes('=')) continue;
    const i = line.indexOf('=');
    if (line.slice(0, i) === key) found = line.slice(i + 1);
  }
  return found;
}

/** Set/replace a property in raw text, preserving other lines; appends if absent. */
export function setProperty(raw: string, key: string, value: string): string {
  const lines = raw.length ? raw.split(/\r?\n/) : [];
  let replaced = false;
  const out = lines.map((line) => {
    const t = line.trimStart();
    if (t.startsWith('#') || !t.includes('=')) return line;
    const i = line.indexOf('=');
    if (line.slice(0, i) === key) {
      replaced = true;
      return `${key}=${value}`;
    }
    return line;
  });
  if (!replaced) {
    // insert before a trailing empty line if present, else append
    if (out.length && out[out.length - 1] === '') out.splice(out.length - 1, 0, `${key}=${value}`);
    else out.push(`${key}=${value}`);
  }
  return out.join('\n');
}

/**
 * Merge edited `values` onto raw `server.properties` text. A key is written when
 * it is already present in `raw` OR its value differs from `defaults[key]`;
 * absent keys left at their default are skipped so the file stays minimal.
 * Unknown keys already in `raw` are preserved (order + comments intact).
 */
export function buildPropertiesText(
  raw: string,
  values: Record<string, string>,
  defaults: Record<string, string>,
): string {
  let out = raw;
  for (const [key, value] of Object.entries(values)) {
    const present = getProperty(raw, key) !== null;
    const isDefault = defaults[key] !== undefined && value === defaults[key];
    if (present || !isDefault) {
      out = setProperty(out, key, value);
    }
  }
  return out;
}
