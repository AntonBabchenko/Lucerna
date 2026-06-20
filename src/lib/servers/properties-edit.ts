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
