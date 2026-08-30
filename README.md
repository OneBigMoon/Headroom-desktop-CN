# Headroom Local Community

> GitHub 项目：[`OneBigMoon/Headroom-macos`](https://github.com/OneBigMoon/Headroom-macos)

Headroom Local Community is an unofficial, local-only desktop edition derived
from the MIT-licensed
[`gglucass/headroom-desktop`](https://github.com/gglucass/headroom-desktop)
project and other open-source components. It is not affiliated with or endorsed
by any upstream paid Headroom product.

## 下载

从 [GitHub Releases](https://github.com/OneBigMoon/Headroom-macos/releases/latest) 下载最新稳定版 macOS DMG。

This fork keeps the open-source local proxy, client connectors, token and
savings dashboard, RTK, MarkItDown, and local add-on management. It does not
provide or require a Headroom account, email login, trial, subscription,
pricing, checkout, cancellation flow, marketing telemetry, or official update
service.

## Isolation from the upstream paid app

Community uses its own runtime identity and never installs over the official
application:

| Resource | Community value |
|---|---|
| Product name | `Headroom Local Community` |
| Bundle ID | `org.headroomlocal.community` |
| App data | `HeadroomLocalCommunity` |
| headroom-ai workspace | `~/.headroom-local-community` |
| Intercept proxy | `127.0.0.1:6867` |
| Managed backend range | `6868-6890` |
| Codex provider / MCP server | `headroom_local_community` |
| Managed markers | `headroom-local-community:*` |

The application does not delete the paid app's bundle data, keychain entries,
`~/.headroom` workspace, managed markers, MCP entry, backups, or shared
HuggingFace cache. The sole read-only exception is
`~/.headroom/mcp_installs.json`, which may be read for the Serena ownership
fingerprint; the official ledger is never modified. Official routing is never
overwritten, and user-managed Serena entries are left untouched.

Codex, Claude Code, OpenCode, and other clients retain their own upstream API
key or OAuth authentication. The proxy and headroom-ai state stay local.

## How it works

```text
Claude Code / Codex / OpenCode
  | ANTHROPIC_BASE_URL=http://127.0.0.1:6867
  | OPENAI_BASE_URL=http://127.0.0.1:6867/v1
  v
Rust intercept proxy :6867
  v
Managed headroom-ai backend :6868-6890
  v
The same upstream API selected by the coding client
```

Anonymous headroom-ai telemetry, Sentry, Aptabase, account APIs, and the
official updater are disabled in this edition.

## Local components

| Component | License | Purpose |
|---|---|---|
| Headroom Desktop upstream | MIT | Tauri desktop shell and connectors |
| headroom-ai | Apache-2.0 | Local prompt optimization proxy |
| RTK | Apache-2.0 | Compact terminal command output |
| MarkItDown | MIT | Convert supported documents to Markdown |

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for attribution and
license details.

## Build on macOS

Requirements: macOS 14 or newer, Node.js/npm, Rust, and Tauri prerequisites.

```bash
npm ci
npm run build:mac:local
```

Artifacts are written under `src-tauri/target/release/bundle`. The local
script validates the frontend and Rust build, creates an ad-hoc signed `.app`
and DMG, and does not install, publish, or replace another application.

## Data written by Community

Depending on enabled features, Community may write its own app data, logs,
preferences, caches, `~/.headroom-local-community`, Community-managed fenced
blocks in client configuration, Serena usage hints in detected
`~/.codex/AGENTS.md` or `~/.claude/CLAUDE.md`, Community-named MCP entries and
guard hooks, and timestamped `.headroom-local-community-backup-*` files before
editing client configuration. Uninstall removes only Community-owned resources
and reverses only Community-managed client changes.

## License

This repository remains available under the upstream MIT license. Preserve the
upstream copyright notice, license, and applicable third-party notices in
redistributions. “Headroom Local Community” must be presented as an unofficial
community edition, not as the upstream paid product.

The project does not provide Homebrew installation. Use the GitHub Releases
DMG linked above.
