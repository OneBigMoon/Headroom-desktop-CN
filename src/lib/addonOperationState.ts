export type AddonOperationMessages = Record<string, string>;

export function setAddonOperationMessage(
  current: AddonOperationMessages,
  id: string,
  message: string,
): AddonOperationMessages {
  return { ...current, [id]: message };
}

export function clearAddonOperationMessage(
  current: AddonOperationMessages,
  id: string,
): AddonOperationMessages {
  if (!(id in current)) return current;
  const next = { ...current };
  delete next[id];
  return next;
}
