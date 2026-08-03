# Beta smoke test

After installing a new beta (`-rc.N`) build, paste this file into Claude Code and ask it to run the checks. Each check has a single expected signal — if any fail, stop and investigate before promoting to stable.

## Setup

1. Quit and relaunch Headroom from Applications.
2. Confirm the tray icon appears in the menu bar.
3. Open the dashboard window once (so the proxy is fully booted).

## Checks (Claude Code pass)

Run these from a Claude Code session and report PASS / FAIL with the observed value. Checks 1, 5, 8, 9, and 10 are client-agnostic — run them once in either client. Codex has very different wiring (no RTK, no `~/.claude/settings.json`, pay-per-token), so its equivalents of checks 6 and 7 live in the **Codex pass** below; run that whole section from a Codex session.

### 1. Version matches the new beta
```bash
ls ~/Applications/Headroom.app/Contents/Info.plist >/dev/null && \
  /usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" /Applications/Headroom.app/Contents/Info.plist
```
Expect: the `-rc.N` version you just installed.

### 2. Proxy is intercepting this conversation
Send a trivial prompt ("say hi"), then:
```bash
stat -f '%Sm' ~/Library/Application\ Support/Headroom/config/activity-facts.json
```
Expect: mtime within the last minute. `lastTransformation` inside the file is a "Recent large compression" tile pick (gated on >=1000 tokens saved and >20% savings, see `activity_facts.rs`), not a per-request heartbeat — don't use it as a liveness signal.

### 3. RTK is on PATH and reports savings (Claude Code only — RTK does not rewrite Codex)
First check whether RTK is installed at all. It is an opt-in addon and can be turned off from the Optimize view, which sets `rtkDisabled` in the setup state; `ensure_rtk_integrations` then writes no PATH block and no Claude Code hook (see `is_rtk_disabled` in `client_adapters.rs`). Skip the check when it is off — a missing `rtk` is the correct state, not a regression.
```bash
jq -r 'if .rtkDisabled then "RTK DISABLED — skip this check" else "RTK enabled — run check" end' \
  ~/Library/Application\ Support/Headroom/config/client-setup.json
```
If enabled:
```bash
zsh -lc 'rtk --version && rtk gain | head -5'
```
Expect: a version line and a gain summary, no "command not found". The `zsh -lc` wrapper is required: `rtk` is added to PATH by the `headroom:managed_rtk` block in `~/.zprofile`, which only a login shell sources. Claude Code's Bash tool (and Codex's shell tool) spawn a non-login, non-interactive shell that does *not* source it, so a bare `rtk` here reports `command not found` on a perfectly healthy install. A login shell exercises the same PATH wiring a real terminal gets, so this confirms both that the managed block is intact and that the binary runs.

### 4. MCP retrieve tool is available (Claude Code only; only if memory tools are enabled)
First check whether the proxy was started with memory tools:
```bash
ls ~/Library/Application\ Support/Headroom/headroom/logs/ | grep -E 'no-memory-tools' >/dev/null && echo 'memory tools DISABLED — skip this check' || echo 'memory tools enabled — run check'
```
If enabled, have Claude call `mcp__headroom__headroom_retrieve` with any small query and expect a tool result (not "No such tool available").

### 5. Tray → Dashboard renders
Click the tray icon, open the dashboard. Expect savings chart and per-client stats render without a blank/error state.

### 6. Pause / resume cleanly strips and restores interception
In Settings, toggle Pause then Resume (restore runs on a background thread, so give it a second), checking after each:
```bash
grep -c 'headroom:claude_code' ~/.zprofile ~/.zshrc
```
Expect: after Pause both files print `0`; after Resume both print `2` (the `# >>> headroom:claude_code >>>` and `# <<< headroom:claude_code <<<` marker lines). Do *not* grep `~/.claude/settings.json` for `headroom-rtk-rewrite` — that hook only exists when the RTK addon is installed, so on an install with RTK off (check 3) it reads `0` in both states and the check looks like a FAIL on a healthy build. The managed shell block is the RTK-independent marker. If RTK *is* installed, `grep -c headroom-rtk-rewrite ~/.claude/settings.json` is a valid extra signal: `0` after Pause, `1` after Resume.

This verifies the Claude Code config only — Pause clears *all* clients, so check C4 in the Codex pass confirms Codex's config is stripped and restored too.

### 7. Proxy is actively optimizing this conversation (not just a heartbeat)
The proxy always runs in `token` mode now (`HEADROOM_MODE=token`, hardcoded — cache mode and the old auth-based mode auto-switch were removed; see `tool_manager.rs`). So `.summary.mode` reports `token` for *every* session, including a Claude Code subscription/OAuth one — don't branch on it. What actually differs per request is the **compression policy**, chosen by the auth-mode classifier (`classify_auth_mode` in the proxy) from the client `User-Agent`:

- **Claude Code subscription/OAuth** (UA `claude-code/`, the normal desktop case) is classified `SUBSCRIPTION` → conservative policy (`live_zone_only=True`, cache-aligner off). The proxy only compresses the **uncached live zone** and freezes the already-cached prefix. Which counter moves depends on the live zone: a request with any live-zone savings counts in `requests_compressed`, while `uncompressed_requests.prefix_frozen` only counts requests the pipeline returned fully unchanged — zero savings (see `build_session_summary` in the proxy's `cost.py`). The right liveness signal here is `cache_savings_usd` plus movement in *either* counter.
- **Pay-per-token API-key / Codex traffic** is classified `PAYG`/`OAUTH` → aggressive policy, so `requests_compressed` and `total_tokens_removed` move directly.

This policy gate is itself guarded by `HEADROOM_PROXY_AUTH_MODE_POLICY_ENFORCEMENT=enabled` (pinned explicitly in `tool_manager.rs`). If that ever reads disabled, subscription traffic silently falls back to the PAYG-aggressive policy and starts busting the prefix cache — a net loss on cache-billed sessions. Pick the sub-check matching the traffic you're driving.

Timing matters either way: a `Read` result becomes part of Claude's *next* outgoing prompt, not the one currently being composed. So the baseline capture, the large Read, and the re-check cannot all happen in one turn — the re-check will still show the old numbers.

**Claude Code subscription/OAuth traffic** (UA `claude-code/`, classified `SUBSCRIPTION`):
1. Capture the baseline:
   ```bash
   rtk proxy curl -s http://127.0.0.1:6767/stats | jq '{primary_model: .summary.primary_model, prefix_frozen: .summary.uncompressed_requests.prefix_frozen, requests_compressed: .summary.compression.requests_compressed, cache_savings_usd: .summary.cost.breakdown.cache_savings_usd, total_tokens_before: .summary.compression.total_tokens_before_with_cli_filtering}'
   ```
2. End the turn with a large Read in flight — e.g. ask Claude to read a long file like `src-tauri/src/lib.rs` with as large an offset/limit window as the Read tool allows (the 25k-token cap means you cannot read it whole; ~1300-1500 lines is plenty).
3. On the *next* turn, re-run the same `jq` command.

Expect: `primary_model` is a `claude-*` model, `cache_savings_usd` is strictly greater (the cached prefix was preserved, not busted), `total_tokens_before` jumped by at least the size of the Read, and `prefix_frozen` + `requests_compressed` together increased by at least 1. The large Read all but guarantees live-zone savings, so in practice the increment lands in `requests_compressed`; `prefix_frozen` only counts requests returned fully unchanged, so it can legitimately stay flat for a whole session (observed on a healthy 0.6.9-rc.1: `prefix_frozen` flat at 17 while cache savings climbed). A bumped mtime on `activity-facts.json` is not enough — interception alone would still touch that file without delivering savings.

**Pay-per-token API-key traffic** (classified `PAYG`/`OAUTH` — this is also the branch Codex hits; the Codex pass below adds a Codex-attributed version):
1. Capture the baseline:
   ```bash
   rtk proxy curl -s http://127.0.0.1:6767/stats | jq '.summary.compression.requests_compressed, .summary.compression.total_tokens_removed'
   ```
2. End the turn with the same large Read in flight (~1300-1500 lines clears the compression threshold).
3. On the *next* turn, re-run the same `jq` command.

Expect: `requests_compressed` increased by at least 1, and `total_tokens_removed` is strictly greater.

### 8. Bundled runtime is healthy
The desktop ships its own Python venv and `headroom` CLI; if either is broken, the proxy can't start cleanly on a fresh install.
```bash
~/Library/Application\ Support/Headroom/headroom/runtime/venv/bin/headroom --version && \
  ~/Library/Application\ Support/Headroom/headroom/runtime/venv/bin/python3 -c "import headroom; print(headroom.__file__)"
```
Expect: a `headroom, version X.Y.Z` line and a path under `.../runtime/venv/lib/python3.12/site-packages/headroom/__init__.py`. No `ModuleNotFoundError`, no `pydantic-core` mismatch traceback (see `extract_required_pydantic_core_version` in `tool_manager.rs` for the exact failure mode).

### 9. Backend port fallback when 6768 is held
The desktop's internal proxy port (default `6768`) can be claimed by other macOS processes — most often `rapportd` at login. The desktop should scan `6769..=6790` and pick a free one instead of failing.

First, confirm the live port and verify the proxy answers there:
```bash
lsof -iTCP -sTCP:LISTEN -nP 2>/dev/null | awk '$1 ~ /(headroom|python)/ && $9 ~ /:(67[6-9][0-9]|6790)/ { print $9 }'
curl -sS -o /dev/null -w '%{http_code}\n' "http://127.0.0.1:6767/livez"
```
Expect: at least one `127.0.0.1:67XX` line in the 6768-6790 range, and the curl returns `200`.

Then, force a fallback. Quit Headroom, hold 6768 with a Python blocker (`nc -l` exits after one connection, so the proxy's first probe frees the port before fallback can trigger), relaunch, and confirm the proxy comes up on a different port. Two timing traps here. First: the quit must wait for the process to actually die — teardown takes 2s+ (`stop_headroom`'s bounded SIGTERM wait plus Codex thread retagging), and `open -a` against a still-dying instance just activates it, so nothing relaunches and the check strands with no app running (a fixed `sleep 2` loses this race; the executable is `headroom-desktop`, not `Headroom`, so poll with `pgrep -x headroom-desktop`). Second: the proxy on a fallback port boots cold (memory tools / model load), so poll `/livez` for up to 90s instead of a fixed sleep:
```bash
osascript -e 'quit app "Headroom"' 2>/dev/null
for _ in $(seq 1 30); do pgrep -xq headroom-desktop || break; sleep 0.5; done
python3 -c "import socket,time; s=socket.socket(); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1); s.bind(('127.0.0.1',6768)); s.listen(16); time.sleep(180)" &
BLOCK_PID=$!
sleep 1
open -a Headroom
for _ in $(seq 1 90); do
  code=$(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:6767/livez" 2>/dev/null)
  [ "$code" = "200" ] && break
  sleep 1
done
echo "livez=$code"
lsof -iTCP -sTCP:LISTEN -nP 2>/dev/null | awk -v IGNORECASE=1 '$1 ~ /(headroom|python)/ && $9 ~ /:(67[6-9][0-9]|6790)/ { print $9 }'
kill $BLOCK_PID 2>/dev/null
```
Expect: `livez=200`, a `127.0.0.1:67XX` line where `XX` is NOT `68` (the fallback worked). A second confirmation is the proxy log *filename*, which embeds the chosen port — a successful fallback leaves a `headroom-proxy---port-6769---....log` next to the usual `...port-6768...` one:
```bash
ls -t ~/Library/Application\ Support/Headroom/headroom/logs/ | grep -m3 'headroom-proxy---port-'
```
After the test, restore the default port with the same wait-for-exit pattern (this relaunch loses the teardown race even more often, because the fallback instance was just cold-booted):
```bash
osascript -e 'quit app "Headroom"' 2>/dev/null
for _ in $(seq 1 30); do pgrep -xq headroom-desktop || break; sleep 0.5; done
open -a Headroom
```

If the fallback is missing, check the desktop log (`logging::log_path()`) for a `[backend_port]` warning line naming the occupant and the chosen fallback port. Note that the *proxy* logs under `.../headroom/logs/` never carry that line — it is emitted by the Rust side, so grepping the proxy log directory for `backend_port` comes back empty even on a successful fallback.

### 10. Auth / pricing state is intact
The session token lives in the macOS keychain under service `com.extraheadroom.headroom.account`, account `session-token`; the local pricing state lives next to `activity-facts.json`.
```bash
security find-generic-password -s com.extraheadroom.headroom.account -a session-token >/dev/null 2>&1 && echo 'signed in' || echo 'not signed in'
test -f ~/Library/Application\ Support/Headroom/config/headroom-pricing-state.json && jq -e '.first_seen_at' ~/Library/Application\ Support/Headroom/config/headroom-pricing-state.json
```
Expect: if the build is supposed to be signed in, line 1 reports `signed in`; line 2 prints a non-null `first_seen_at` timestamp. A signed-in build that flips to `not signed in` after relaunch is a regression — keychain access is broken or the token was wiped.

## Codex checks (Codex pass)

Run these from a Codex CLI session (or with Codex configured and at least one Codex prompt sent this session). Codex routes through Headroom via an `OPENAI_BASE_URL` shell export plus a managed provider block in `~/.codex/config.toml` — not `~/.claude/settings.json` and not RTK — and its traffic is pay-per-token, so the proxy runs it in `token` mode.

### C1. Codex is configured to route through Headroom
```bash
grep -q 'model_provider = "headroom"' ~/.codex/config.toml && \
  grep -q 'openai_base_url = "http://127.0.0.1:6767/v1"' ~/.codex/config.toml && \
  grep -qF '[model_providers.headroom]' ~/.codex/config.toml && \
  grep -q 'supports_websockets = false' ~/.codex/config.toml && \
  grep -q 'export OPENAI_BASE_URL=http://127.0.0.1:6767/v1' ~/.zshrc ~/.zprofile 2>/dev/null && \
  echo PASS || echo FAIL
```
Expect: `PASS`. `~/.codex/config.toml` carries both managed marker blocks — `# >>> headroom:codex_cli >>>` with the root `model_provider`/`openai_base_url` keys, and `# >>> headroom:codex_cli_provider >>>` with the `[model_providers.headroom]` table — and a managed shell block exports `OPENAI_BASE_URL`. Headroom deliberately keeps `supports_websockets = false` so Codex uses the reliable HTTP Responses stream instead of failing the whole turn when an upstream WebSocket closes before `response.completed`. A `FAIL` means setup didn't write one of them (see `configure_codex_provider_block` / `configure_shell_block` in `client_adapters.rs`).

### C2. Codex traffic is actively optimized (token mode)
Codex is billed per token, so unlike a Claude Code subscription it runs in `token` mode and `requests_compressed` *does* move. Run this from inside Codex.
1. Capture the baseline:
   ```bash
   rtk proxy curl -s http://127.0.0.1:6767/stats | jq '{mode: .summary.mode, primary_model: .summary.primary_model, requests_compressed: .summary.compression.requests_compressed, total_tokens_removed: .summary.compression.total_tokens_removed}'
   ```
2. End the turn with a large file read in flight from Codex (~1300-1500 lines clears the compression threshold). As in check 7, the read lands in Codex's *next* prompt, so the re-check must be on a later turn.
3. On the next turn, re-run the same command.

Expect: `mode` is `token`, `primary_model` is a `gpt-*` model (confirms Codex — not Claude — is the traffic being measured), `requests_compressed` increased by at least 1, and `total_tokens_removed` is strictly greater. If `primary_model` is a `claude-*` model, the proxy is dominated by Claude traffic — confirm the prompt actually ran through Codex before trusting this check.

### C3. Codex savings are attributed on the dashboard
Open the dashboard and confirm a **Codex** group appears in the per-provider savings with non-zero values. Provider `openai` maps to the Codex group (`mergeProviderSavingsForDisplay` in `dashboardHelpers.ts`); a missing Codex group after Codex traffic means per-provider attribution isn't tagging OpenAI requests.

### C4. Pause / resume cleanly strips and restores Codex routing
The Claude equivalent is check 6; Pause clears *all* client setups, so it must remove Codex's config too. In Settings, toggle Pause then Resume (restore runs on a background thread, so give it a second), checking after each:
```bash
grep -c 'headroom:codex_cli' ~/.codex/config.toml
cat ~/.zshrc ~/.zprofile 2>/dev/null | grep -c 'OPENAI_BASE_URL=http://127.0.0.1:6767'
```
Expect: after Pause both print `0`; after Resume both are non-zero (config.toml back to `4` marker lines, shell back to one export per managed profile). Pause routes through `disable_codex_cli` — strips both TOML blocks, the `openai_base_url` root key, and the shell blocks; Resume re-applies them via `restore_client_setups`.

## Inspecting the proxy directly

When inspecting the running proxy by hand (e.g. checking `/stats`), wrap `curl` with `rtk proxy` to bypass RTK's output filtering — otherwise large JSON responses get summarized into a type-shape view that looks like a broken endpoint:

```bash
rtk proxy curl -s http://127.0.0.1:6767/stats | jq .summary
```

Every `rtk` invocation in this doc (checks 3, 7, C2, and above) has the same PATH caveat as check 3: when Claude Code or Codex runs them through their shell tool, `rtk` is not on PATH because the non-login shell never sources `~/.zprofile`. Either wrap the command in `zsh -lc '...'`, or call the binary by its managed path:

```bash
"$HOME/Library/Application Support/Headroom/headroom/bin/rtk" proxy curl -s http://127.0.0.1:6767/stats | jq .summary
```

## When something fails

- Proxy log silent → check `~/Library/Application Support/Headroom/headroom/logs/` for a newer log file or a crash file.
- RTK missing → check the managed block in `~/.zshrc` / `~/.zprofile` is intact and the shell has been reloaded.
- MCP tool missing → restart Claude Code; the MCP server registration happens at session start.
