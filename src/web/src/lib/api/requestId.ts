/** UUIDv7: 48-bit millisecond timestamp, then random bits, with the version and variant bits set. */
export function uuidV7(
  nowMs: number = Date.now(),
  random: Uint8Array = crypto.getRandomValues(new Uint8Array(10)),
): string {
  const bytes = new Uint8Array(16);
  const stamp = BigInt(nowMs);
  for (let i = 0; i < 6; i++) {
    bytes[i] = Number((stamp >> BigInt(8 * (5 - i))) & 0xffn);
  }
  bytes.set(random.subarray(0, 10), 6);
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x70;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
