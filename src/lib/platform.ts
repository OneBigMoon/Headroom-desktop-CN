export const PREVIEW_SUPPORT_EMAIL = "headroom-local-community@localhost.invalid";

export function platformPreviewNoticeFor(
  platform: string | undefined,
  supportTier: string | undefined,
): string | null {
  if (supportTier !== "experimental") {
    return null;
  }
  if (platform === "linux" || platform === "windows") {
    const name = platform === "linux" ? "Linux" : "Windows";
    return `${name} is currently in preview.`;
  }
  return "This platform is currently in preview.";
}

/** Pre-filled support mail for preview-platform reports: the diagnostics we
 *  always end up asking for (platform, app build, runtime build) are already
 *  in the body so the user only has to describe what broke. */
export function platformPreviewSupportMailto(context: {
  platform: string | undefined;
  appVersion: string;
  headroomVersion: string;
}): string {
  const subject = `Headroom ${context.platform ?? "platform"} preview issue`;
  const body =
    "What happened, and what were you doing at the time?\n\n\n" +
    "---\n" +
    "Diagnostic info (please keep):\n" +
    [
      `Platform: ${context.platform ?? "unknown"}`,
      `App version: ${context.appVersion}`,
      `Headroom CLI: ${context.headroomVersion}`,
    ].join("\n");
  return `mailto:${PREVIEW_SUPPORT_EMAIL}?subject=${encodeURIComponent(
    subject,
  )}&body=${encodeURIComponent(body)}`;
}
