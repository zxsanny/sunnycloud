const UNITS = ['B', 'kB', 'MB', 'GB', 'TB', 'PB'];

/** Decimal units (1 GB = 10^9 bytes), one decimal from MB up: `38.2 GB`. */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const digits = unit < 2 ? 0 : 1;
  return `${value.toFixed(digits)} ${UNITS[unit]}`;
}
