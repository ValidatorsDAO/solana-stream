const toSlotNumber = (value: unknown): number | null => {
  if (typeof value !== 'string' && typeof value !== 'number') {
    return null
  }
  const slot = Number(value)
  return Number.isFinite(slot) ? slot : null
}

export const getUpdateSlot = (update: any): number | null => {
  return (
    toSlotNumber(update?.transaction?.slot) ??
    toSlotNumber(update?.account?.slot) ??
    toSlotNumber(update?.slot?.slot) ??
    toSlotNumber(update?.block?.slot) ??
    toSlotNumber(update?.blockMeta?.slot) ??
    toSlotNumber(update?.entry?.slot)
  )
}
