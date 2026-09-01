import { describe, expect, it } from "vitest";
import type { Translate, TranslationKey, TranslationValues } from "./i18n";
import {
  PROJECT_ISSUES_NEW_URL,
  platformPreviewNoticeFor,
  platformPreviewSupportIssueUrl,
} from "./platform";

const messages: Partial<Record<TranslationKey, string>> = {
  "platform.previewNamed": "{platform} is currently in preview.",
  "platform.previewGeneric": "This platform is currently in preview.",
  "platform.issueSubject": "Headroom {platform} preview issue",
  "platform.issuePrompt": "What happened, and what were you doing at the time?",
  "platform.issueDiagnostics": "Diagnostic info (please keep):",
  "platform.issuePlatform": "Platform: {platform}",
  "platform.issueAppVersion": "App version: {version}",
  "platform.issueCliVersion": "Headroom CLI: {version}",
  "platform.unknown": "unknown",
};

const t: Translate = (key, values?: TranslationValues) => {
  let message = messages[key] ?? key;
  for (const [name, value] of Object.entries(values ?? {})) {
    message = message.split(`{${name}}`).join(String(value));
  }
  return message;
};

describe("platformPreviewNoticeFor", () => {
  it("returns null when the platform is stable", () => {
    expect(platformPreviewNoticeFor("macos", "stable", t)).toBeNull();
  });

  it("returns the linux message for experimental linux", () => {
    expect(platformPreviewNoticeFor("linux", "experimental", t)).toContain("Linux");
  });

  it("returns the windows message for experimental windows", () => {
    expect(platformPreviewNoticeFor("windows", "experimental", t)).toContain("Windows");
    expect(platformPreviewNoticeFor("win32", "experimental", t)).toContain("Windows");
  });

  it("returns the generic message for other experimental platforms", () => {
    expect(platformPreviewNoticeFor("freebsd", "experimental", t)).toBe(
      "This platform is currently in preview.",
    );
  });
});

describe("platformPreviewSupportIssueUrl", () => {
  it("opens the project issue form with platform and version diagnostics", () => {
    const issueUrl = platformPreviewSupportIssueUrl(
      {
        platform: "windows",
        appVersion: "0.8.4",
        headroomVersion: "0.35.1",
      },
      t,
    );

    const url = new URL(issueUrl);
    expect(`${url.origin}${url.pathname}`).toBe(PROJECT_ISSUES_NEW_URL);
    expect(url.searchParams.get("title")).toContain("windows preview issue");
    const body = url.searchParams.get("body") ?? "";
    expect(body).toContain("Platform: windows");
    expect(body).toContain("App version: 0.8.4");
    expect(body).toContain("Headroom CLI: 0.35.1");
  });

  it("does not emit \"undefined\" when the platform is unknown", () => {
    const url = platformPreviewSupportIssueUrl(
      {
        platform: undefined,
        appVersion: "0.8.4",
        headroomVersion: "unknown",
      },
      t,
    );
    expect(decodeURIComponent(url)).not.toContain("undefined");
  });
});
