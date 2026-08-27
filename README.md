# Headroom Local Community

> GitHub 项目：[`OneBigMoon/Headroom-macos`](https://github.com/OneBigMoon/Headroom-macos)

Headroom Local Community is an unofficial, local-only desktop edition derived
from the MIT-licensed
[`gglucass/headroom-desktop`](https://github.com/gglucass/headroom-desktop)
project. It is not affiliated with or endorsed by the upstream paid Headroom
product.

This fork keeps the open-source local proxy, client connectors, token and
savings dashboard, RTK, MarkItDown, and local add-on management. It does not
provide or require a Headroom account, email login, trial, subscription,
pricing, checkout, cancellation flow, marketing telemetry, or the official
update service.

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
| Codex provider | `headroom_local_community` |
| MCP server | `headroom_local_community` |
| Managed markers | `headroom-local-community:*` |

The application does not read or delete the paid app's bundle data, keychain
entries, `~/.headroom` workspace, managed markers, MCP entry, backups, or
shared HuggingFace cache.

A coding client can have only one active `ANTHROPIC_BASE_URL` or
`OPENAI_BASE_URL`. If official Headroom routing is detected, Community refuses
to overwrite it. Pause that connector in the official app before enabling the
Community connector. Community never removes the official configuration.

Codex, Claude Code, OpenCode, and other clients still use their own upstream
API key or OAuth authentication. Only the separate Headroom product account
and billing system is removed.

## How it works

```text
Claude Code / Codex / OpenCode
        |
        | ANTHROPIC_BASE_URL=http://127.0.0.1:6867
        | OPENAI_BASE_URL=http://127.0.0.1:6867/v1
        v
Rust intercept proxy :6867
        |
        v
Managed headroom-ai backend :6868-6890
        |
        v
The same upstream API selected by the coding client
```

The proxy and headroom-ai state stay local. Anonymous headroom-ai telemetry,
Sentry, Aptabase, account APIs, and the official updater are disabled in this
edition.

## Local components

| Component | License | Purpose |
|---|---|---|
| Headroom Desktop upstream | MIT | Tauri desktop shell and connectors |
| headroom-ai | Apache-2.0 | Local prompt optimization proxy |
| RTK | Apache-2.0 | Compact terminal command output |
| MarkItDown | MIT | Convert supported documents to Markdown |

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for attribution and
license details.

## 致谢与开源声明

感谢原始桌面端作者 [`gglucass`](https://github.com/gglucass/headroom-desktop)，以及 `headroomlabs-ai/headroom`、`rtk-ai/rtk`、Microsoft `MarkItDown` 和各可选开源工具的作者与贡献者。

本 Community 版本的代码复用、工具集成、修改和再分发全部以相应项目的开源许可证为依据。该致谢不表示原作者提供商业授权、官方背书、签名证书、付费服务或维护承诺。完整来源与许可证见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

macOS DMG 安装、ad-hoc 签名、quarantine 处理、Gatekeeper 验证及自动更新密钥说明见 [macOS 安装、签名与自动更新说明](docs/macos-install-and-signing.zh-CN.md)。

## Build on macOS

Requirements:

- macOS 14 or newer
- Node.js and npm
- Rust toolchain
- Tauri system prerequisites

Install dependencies and build an unsigned local package:

```bash
npm ci
npm run build:mac:local
```

Artifacts are written under:

```text
src-tauri/target/release/bundle
```

The build script runs the configured frontend and Rust validation before
creating `.app` and `.dmg` artifacts. It does not install, launch, sign,
notarize, publish, or modify an existing `/Applications/Headroom.app`.

Unsigned builds may trigger macOS Gatekeeper warnings. Public distribution
without those warnings requires your own Apple Developer ID signing identity
and Apple notarization. Do not use the upstream author's signing identity or
official update channel.

## Development

```bash
npm ci
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm run test:frontend
```

Do not start Community during development when official Headroom is actively
routing the same coding clients. Tests should use temporary homes and isolated
configuration fixtures.

## Data written by Community

Depending on platform and enabled features, Community may write:

- Its own Tauri app data, logs, preferences, and caches under the Community
  product name and bundle ID.
- `~/.headroom-local-community` for headroom-ai runtime state.
- Community-managed, fenced blocks in supported client configuration files.
- Community-named MCP entries and guard hooks.
- Timestamped backups ending in `.headroom-local-community-backup-*` before
  editing a client configuration file.

The in-app uninstall flow removes only those Community-owned resources and
reverses only Community-managed client changes.

## License

This repository remains available under the upstream MIT license. Preserve the
upstream copyright notice, this license, and applicable third-party notices in
redistributions. “Headroom Local Community” must be presented as an unofficial
community edition, not as the upstream paid product.
