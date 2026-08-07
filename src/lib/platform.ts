export function platformPreviewNoticeFor(
  platform: string | undefined,
  supportTier: string | undefined,
): string | null {
  if (supportTier !== "experimental") {
    return null;
  }
  if (platform === "linux") {
    return "Linux is currently a preview build. Core proxy routing is supported, but Headroom Learn and secure API key storage are disabled while the platform is hardened.";
  }
  if (platform === "windows") {
    return "Windows is currently a preview build. Core proxy routing is supported while the platform is hardened; Headroom Learn stays enabled.";
  }
  return "This platform is currently in preview.";
}
