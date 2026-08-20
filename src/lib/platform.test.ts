import { describe, expect, it } from "vitest";
import {
  platformPreviewNoticeFor,
  platformPreviewSupportMailto,
} from "./platform";

describe("platformPreviewNoticeFor", () => {
  it("returns null when the platform is stable", () => {
    expect(platformPreviewNoticeFor("macos", "stable")).toBeNull();
  });

  it("returns the linux message for experimental linux", () => {
    expect(platformPreviewNoticeFor("linux", "experimental")).toContain("Linux");
  });

  it("returns the windows message for experimental windows", () => {
    expect(platformPreviewNoticeFor("windows", "experimental")).toContain("Windows");
  });

  it("returns the generic message for other experimental platforms", () => {
    expect(platformPreviewNoticeFor("freebsd", "experimental")).toBe(
      "This platform is currently in preview.",
    );
  });
});

describe("platformPreviewSupportMailto", () => {
  it("carries the platform and both versions in the decoded body", () => {
    const url = platformPreviewSupportMailto({
      platform: "windows",
      appVersion: "0.8.4",
      headroomVersion: "0.35.1",
    });
    expect(url.startsWith("mailto:support@extraheadroom.com?subject=")).toBe(true);
    const body = decodeURIComponent(url.split("&body=")[1]);
    expect(body).toContain("Platform: windows");
    expect(body).toContain("App version: 0.8.4");
    expect(body).toContain("Headroom CLI: 0.35.1");
  });

  it("does not emit \"undefined\" when the platform is unknown", () => {
    const url = platformPreviewSupportMailto({
      platform: undefined,
      appVersion: "0.8.4",
      headroomVersion: "unknown",
    });
    expect(decodeURIComponent(url)).not.toContain("undefined");
  });
});
