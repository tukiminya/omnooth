const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];

/** Decimal file sizes, rounded to one decimal place with unnecessary zeros omitted. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  let rounded = Number(value.toFixed(unit === 0 ? 0 : 1));
  if (rounded >= 1000 && unit < units.length - 1) {
    rounded = Number((rounded / 1000).toFixed(1));
    unit += 1;
  }
  return `${rounded} ${units[unit]}`;
}
