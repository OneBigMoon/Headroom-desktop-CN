#!/usr/bin/env node

import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const STABLE_VERSION = /^\d+\.\d+\.\d+$/;
const REPOSITORY = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;

function requireFile(path, label) {
  const contents = readFileSync(path);
  if (contents.length === 0) {
    throw new Error(`${label} is empty: ${path}`);
  }
  return contents;
}

export function prepareCommunityRelease({
  version,
  tag,
  repository,
  dmgPath,
  updaterPath,
  signaturePath,
  notesPath,
  outputDir,
  pubDate = new Date().toISOString(),
}) {
  if (!STABLE_VERSION.test(version)) {
    throw new Error(`Stable release version required, got: ${version}`);
  }
  if (tag !== `v${version}`) {
    throw new Error(`Tag ${tag} does not match version ${version}`);
  }
  if (!REPOSITORY.test(repository)) {
    throw new Error(`Invalid GitHub repository: ${repository}`);
  }
  if (Number.isNaN(Date.parse(pubDate))) {
    throw new Error(`Invalid publication date: ${pubDate}`);
  }

  requireFile(dmgPath, "DMG");
  requireFile(updaterPath, "Updater archive");
  const signature = requireFile(signaturePath, "Updater signature")
    .toString("utf8")
    .trim();
  const notes = requireFile(notesPath, "Release notes").toString("utf8").trim();
  if (!signature) {
    throw new Error(`Updater signature is empty: ${signaturePath}`);
  }
  if (!notes) {
    throw new Error(`Release notes are empty: ${notesPath}`);
  }

  const stagingDir = resolve(outputDir);
  const assetsDir = join(stagingDir, "assets");
  mkdirSync(assetsDir, { recursive: true });

  const dmgName = `Headroom.Local.Community_${version}_universal.dmg`;
  const updaterName = "Headroom.Local.Community_universal.app.tar.gz";
  const signatureName = `${updaterName}.sig`;

  copyFileSync(dmgPath, join(assetsDir, dmgName));
  copyFileSync(updaterPath, join(assetsDir, updaterName));
  copyFileSync(signaturePath, join(assetsDir, signatureName));

  const updaterUrl = `https://github.com/${repository}/releases/download/${tag}/${updaterName}`;
  const platform = { signature, url: updaterUrl };
  const manifest = {
    version,
    notes,
    pub_date: new Date(pubDate).toISOString(),
    platforms: {
      "darwin-aarch64": platform,
      "darwin-x86_64": platform,
      "darwin-aarch64-app": platform,
      "darwin-x86_64-app": platform,
    },
  };

  writeFileSync(join(assetsDir, "latest.json"), `${JSON.stringify(manifest)}\n`);
  writeFileSync(join(stagingDir, "release-body.md"), `${notes}\n`);

  return {
    assetsDir,
    assetNames: [dmgName, updaterName, signatureName, "latest.json"],
    manifest,
  };
}

function main(argv) {
  if (argv.length !== 8) {
    throw new Error(
      "Usage: prepare-community-release.mjs <version> <tag> <owner/repo> <dmg> <updater.tar.gz> <signature> <notes.md> <output-dir>",
    );
  }

  const [version, tag, repository, dmgPath, updaterPath, signaturePath, notesPath, outputDir] =
    argv;
  const result = prepareCommunityRelease({
    version,
    tag,
    repository,
    dmgPath,
    updaterPath,
    signaturePath,
    notesPath,
    outputDir,
  });
  process.stdout.write(`${result.assetNames.join("\n")}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
