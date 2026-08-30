import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { prepareCommunityRelease } from "./prepare-community-release.mjs";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "headroom-release-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));

  const files = {
    dmgPath: join(root, "input.dmg"),
    updaterPath: join(root, "input.app.tar.gz"),
    signaturePath: join(root, "input.app.tar.gz.sig"),
    notesPath: join(root, "notes.md"),
    outputDir: join(root, "staging"),
  };
  writeFileSync(files.dmgPath, "dmg");
  writeFileSync(files.updaterPath, "updater");
  writeFileSync(files.signaturePath, "signature");
  writeFileSync(files.notesPath, "Fixed updater retry.\n");
  return files;
}

test("stages the four updater assets with stable release metadata", (t) => {
  const result = prepareCommunityRelease({
    version: "1.2.3",
    tag: "v1.2.3",
    repository: "OneBigMoon/Headroom-macos",
    pubDate: "2026-08-30T00:00:00.000Z",
    ...fixture(t),
  });

  assert.deepEqual(result.assetNames, [
    "Headroom.Local.Community_1.2.3_universal.dmg",
    "Headroom.Local.Community_universal.app.tar.gz",
    "Headroom.Local.Community_universal.app.tar.gz.sig",
    "latest.json",
  ]);
  assert.equal(result.manifest.version, "1.2.3");
  assert.equal(result.manifest.notes, "Fixed updater retry.");
  assert.deepEqual(Object.keys(result.manifest.platforms), [
    "darwin-aarch64",
    "darwin-x86_64",
    "darwin-aarch64-app",
    "darwin-x86_64-app",
  ]);
  assert.equal(
    result.manifest.platforms["darwin-aarch64"].url,
    "https://github.com/OneBigMoon/Headroom-macos/releases/download/v1.2.3/Headroom.Local.Community_universal.app.tar.gz",
  );
  assert.equal(
    JSON.parse(readFileSync(join(result.assetsDir, "latest.json"), "utf8")).version,
    "1.2.3",
  );
});

test("rejects prerelease versions and mismatched tags", (t) => {
  const files = fixture(t);
  assert.throws(
    () =>
      prepareCommunityRelease({
        version: "1.2.3-rc.1",
        tag: "v1.2.3-rc.1",
        repository: "OneBigMoon/Headroom-macos",
        ...files,
      }),
    /Stable release version required/,
  );
  assert.throws(
    () =>
      prepareCommunityRelease({
        version: "1.2.3",
        tag: "v1.2.4",
        repository: "OneBigMoon/Headroom-macos",
        ...files,
      }),
    /does not match version/,
  );
});

test("release workflow resolves drafts from the release list and updates by id", () => {
  const workflow = readFileSync(
    new URL("../.github/workflows/release-community-macos.yml", import.meta.url),
    "utf8",
  );
  const publishStep = workflow.match(
    /- name: Create draft, verify uploaded digests, and publish([\s\S]*?)- name: Verify immutable public release downloads/,
  );

  assert.ok(publishStep);
  assert.match(publishStep[1], /gh api --paginate --slurp "\$\{releases_api\}"/);
  assert.match(
    publishStep[1],
    /release_api="repos\/\$\{GITHUB_REPOSITORY\}\/releases\/\$\{release_id\}"/,
  );
  assert.match(
    publishStep[1],
    /gh api --method PATCH "\$\{release_api\}" -F draft=false/,
  );
  assert.match(
    publishStep[1],
    /gh api --method PATCH "\$\{release_api\}" -F draft=true/,
  );
  assert.doesNotMatch(publishStep[1], /releases\/tags\/\$\{tag\}/);
});
