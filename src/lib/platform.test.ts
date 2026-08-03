import { describe, expect, it } from "vitest";
import { platformPreviewNoticeFor } from "./platform";

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
