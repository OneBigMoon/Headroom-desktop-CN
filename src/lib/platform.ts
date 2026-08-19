export function platformPreviewNoticeFor(
  platform: string | undefined,
  supportTier: string | undefined,
): string | null {
  if (supportTier !== "experimental") {
    return null;
  }
  if (platform === "linux" || platform === "windows") {
    const name = platform === "linux" ? "Linux" : "Windows";
    return `${name} is currently a preview build. Core proxy routing is supported while the platform is hardened; Headroom Learn stays enabled.`;
  }
  return "This platform is currently in preview.";
}
