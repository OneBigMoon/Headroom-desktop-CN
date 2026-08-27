export type AnalyticsProperties = Record<
  string,
  string | number | boolean | null | undefined
>;

const installMilestonePrefix = "headroom.analytics.install.";
const seenInstallMilestones = new Set<string>();

export function trackAnalyticsEvent(
  _name: string,
  _properties?: AnalyticsProperties
) {
  // Community builds do not send product analytics.
}

// Returns true only on the send that actually fired, so callers can piggyback
// their own once-only side effects on the same dedupe.
export function trackInstallMilestoneOnce(
  name: string,
  properties?: AnalyticsProperties
): boolean {
  const storageKey = `${installMilestonePrefix}${name}`;
  if (seenInstallMilestones.has(storageKey)) {
    return false;
  }

  try {
    if (localStorage.getItem(storageKey) === "1") {
      seenInstallMilestones.add(storageKey);
      return false;
    }
    localStorage.setItem(storageKey, "1");
  } catch {
    // Fall through and at least dedupe for the current session.
  }

  seenInstallMilestones.add(storageKey);
  trackAnalyticsEvent(name, properties);
  return true;
}
