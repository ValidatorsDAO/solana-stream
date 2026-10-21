export type BlockMarkerKind =
  | 'BlockFooter'
  | 'BlockHeader'
  | 'UpdateParent'
  | 'GenesisCertificate'
  | 'Unknown'

const KINDS: readonly BlockMarkerKind[] = [
  'BlockFooter',
  'BlockHeader',
  'UpdateParent',
  'GenesisCertificate',
]

const isZero = (bytes: Uint8Array, from: number, to: number): boolean => {
  for (let i = from; i < to; i++) {
    if (bytes[i] !== 0) return false
  }
  return true
}

/**
 * Classifies a deshredded `entries` payload.
 *
 * Returns `null` for an entry batch (including the empty batch that closes an
 * aborted slot) and the marker kind for an Alpenglow block marker. A payload that
 * starts like a marker but is not a well-formed V1 marker, or carries a variant
 * this SDK does not know, is `'Unknown'`.
 *
 * Layout: entry count 0 (u64 LE) / marker version (u16 LE) / variant (u8) /
 * byte length (u16 LE) / data, optionally followed by zero padding.
 */
export function classifyBlockMarker(
  payload: Uint8Array,
): BlockMarkerKind | null {
  if (payload.length < 8 || !isZero(payload, 0, 8)) return null
  if (isZero(payload, 8, payload.length)) return null

  if (payload.length < 13 || payload[8] !== 1 || payload[9] !== 0) {
    return 'Unknown'
  }
  const end = 13 + (payload[11] | (payload[12] << 8))
  if (end > payload.length || !isZero(payload, end, payload.length)) {
    return 'Unknown'
  }

  return KINDS[payload[10]] ?? 'Unknown'
}
