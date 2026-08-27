# macOS 安装、签名与自动更新说明

Headroom Local Community（GitHub 项目：`OneBigMoon/Headroom-macos`）是非官方社区版本。代码复用、工具集成与再分发全部依据各项目公开的开源许可证，不包含 Headroom 官方付费服务、官方签名证书或官方更新通道。

## 推荐安装方式

1. 从 GitHub Releases 下载与你的 Mac 架构匹配的 DMG。
2. 打开 DMG，把 `Headroom Local Community.app` 拖入“应用程序”。
3. 首次启动优先在 Finder 中右键应用并选择“打开”。

当前社区发行版使用免费的 ad-hoc 本地代码签名，并额外使用独立的 Tauri 更新签名验证自动更新包。它不等同于 Apple Developer ID 签名或 Apple 公证。

## 出现“无法验证开发者”或“应用已损坏”

先把应用复制到 `/Applications`，然后只对这个应用清除下载隔离属性：

```bash
xattr -dr com.apple.quarantine "/Applications/Headroom Local Community.app"
```

验证应用包内部签名：

```bash
codesign --verify --deep --strict --verbose=2 "/Applications/Headroom Local Community.app"
```

如果应用包在复制、解压或二次打包过程中被修改，重新进行 ad-hoc 签名：

```bash
codesign --force --deep --sign - --timestamp=none \
  --entitlements "/Applications/Headroom Local Community.app/Contents/Resources/Entitlements.plist" \
  "/Applications/Headroom Local Community.app"
```

不同构建可能没有把 Entitlements 文件复制进 Resources。此时使用仓库中的文件：

```bash
cd /path/to/Headroom-macos
codesign --force --deep --sign - --timestamp=none \
  --entitlements "src-tauri/Entitlements.plist" \
  "/Applications/Headroom Local Community.app"
xattr -dr com.apple.quarantine "/Applications/Headroom Local Community.app"
codesign --verify --deep --strict --verbose=2 "/Applications/Headroom Local Community.app"
```

检查当前隔离属性：

```bash
xattr -p com.apple.quarantine "/Applications/Headroom Local Community.app"
```

如果命令返回“没有这个属性”，说明隔离标记已经清除。

不要为了运行单个社区应用而全局关闭 Gatekeeper。不要执行 `sudo spctl --master-disable` 作为常规安装步骤。

## 本机从源码构建

```bash
git clone https://github.com/OneBigMoon/Headroom-macos.git
cd Headroom-macos
npm ci
npm run build:mac:local
```

构建脚本会运行前端测试、Rust 测试和生产构建，随后为 `.app` 进行 ad-hoc 签名并创建 DMG：

```text
src-tauri/target/release/bundle/macos/Headroom Local Community.app
src-tauri/target/release/bundle/dmg/Headroom Local Community_<版本>_<架构>.dmg
```

## 自动更新签名

应用更新与 macOS Gatekeeper 是两套不同的验证：

- macOS 代码签名验证 `.app` 是否被修改。
- Tauri updater 签名验证下载的更新包是否由 Community 独立发布密钥签发。

生成一套新的 updater 密钥：

```bash
npx tauri signer generate \
  --write-keys "$HOME/.tauri/headroom-local-community.key"
```

私钥和密码只能保存在发布者本机或 GitHub Secrets，不能提交到仓库。公钥可以写入 `src-tauri/tauri.conf.json` 和应用的默认更新配置。

GitHub Actions 使用的名称：

```text
Secret: TAURI_SIGNING_PRIVATE_KEY
Secret: TAURI_SIGNING_PRIVATE_KEY_PASSWORD
Variable: HEADROOM_UPDATER_PUBLIC_KEY
```

## 正式无警告分发

要让普通用户下载后直接双击安装且不需要清除 quarantine，必须使用发布者自己的 Apple Developer ID Application 证书、Hardened Runtime 和 Apple 公证。Community 不得使用 Headroom 原作者的证书、私钥或官方更新通道。

常用验证命令：

```bash
codesign -dv --verbose=4 "/Applications/Headroom Local Community.app"
spctl --assess --type execute --verbose=4 "/Applications/Headroom Local Community.app"
```

ad-hoc 签名通过 `codesign --verify`，但通常不会通过 `spctl --assess` 的 Developer ID/Gatekeeper 分发评估，这是预期区别。

## 回滚

自动更新安装前请退出正在使用本地代理的编码工具。若新版本异常，可从上一版本 DMG 重新安装；应用数据保存在独立的 `HeadroomLocalCommunity` 目录，不会覆盖官方 Headroom 应用的数据。
