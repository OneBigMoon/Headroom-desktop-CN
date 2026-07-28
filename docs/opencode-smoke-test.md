# OpenCode smoke test

Companion to `beta-smoke-test.md` for the OpenCode connector (Grok Build has its own file, `grok-smoke-test.md`). After installing a beta (`-rc.N`) build that carries it, paste this file into an OpenCode session - or into Claude Code - and ask the agent to run the checks. Each check has a single expected signal - if any fail, stop and investigate before promoting to stable.

The checks drive OpenCode with one-shot commands (`opencode run`), so they work from either agent's shell tool. One-shot runs complete before the command returns, so there is no "next turn" timing trap here (contrast check 7 in the main doc): the proxy log line exists the moment the command exits.

## Setup

1. Run the main `beta-smoke-test.md` setup (relaunch, tray, dashboard open).
2. Confirm the binary exists: `opencode --version` (Homebrew) or `~/.opencode/bin/opencode --version`.
3. Enable the OpenCode connector in Settings if not already enabled.
4. `tail -0 -f ~/.headroom/logs/proxy.log` in a spare terminal helps, but every check below greps the log after the fact, so it is optional.

## Checks

### O1. Config written: both baseURLs + transport plugin

```bash
CFG=~/.config/opencode/opencode.json; [ -f ~/.config/opencode/opencode.jsonc ] && CFG=~/.config/opencode/opencode.jsonc
jq -e '.provider.anthropic.options.baseURL == "http://127.0.0.1:6767/v1"
   and .provider.openai.options.baseURL == "http://127.0.0.1:6767/v1"
   and (.plugin | map(test("Headroom/opencode/entry\\.opencode\\.js$")) | any)' "$CFG" \
  && test -f ~/Library/Application\ Support/Headroom/opencode/entry.opencode.js \
  && echo PASS || echo FAIL
```

Expect: `PASS`. A missing `plugin` entry means `configure_opencode_provider_block` ran an older build; a missing file means `ensure_opencode_plugin_file` failed (both in `client_adapters.rs`).

### O2. Anthropic traffic is classified and optimized

Requires a real Anthropic key (env or `opencode auth`). Run from any directory WITHOUT its own `opencode.json` - a project-level config overrides the proxied global one, which is documented behavior, not a bug.

```bash
opencode run -m anthropic/claude-sonnet-4-5 "say hi"
grep 'client=opencode' ~/.headroom/logs/proxy.log | tail -1
```

Expect: a normal answer, and a `PERF` line with `client=opencode` and `path=/v1/messages`-family stage timing above it. `tok_saved` may legitimately be small on a tiny prompt; the classification (`client=opencode`, not `claude-code`) is the pass signal. For a savings signal, re-run from a repo and ask it to read a large file first.

### O3. OpenAI path rides the same base URL

```bash
opencode run -m openai/gpt-5 "say hi"
grep 'client=opencode' ~/.headroom/logs/proxy.log | tail -1
```

Expect: same `client=opencode` attribution with a `gpt-*` model. This proves the `/v1/responses` path is classified by UA and not mislabeled as Codex (the intercept stamps `X-Client: opencode` before the codex path-based stamp).

### O4. Transport plugin routes third-party providers

No Google key needed - a vendor-worded auth error is the pass signal.

```bash
opencode run -m google/gemini-2.5-flash "say hi" 2>&1 | tail -2
grep 'generativelanguage' ~/.headroom/logs/proxy.log | tail -1
```

Expect: an error mentioning a Google API key (not an OpenAI or Anthropic error - the wording proves which vendor answered), and a proxy log line forwarding to `generativelanguage.googleapis.com` with `client=opencode` and `transforms=none`. `transforms=none` is EXPECTED: third-party formats are routed and attributed but not compressed (upstream issue #2602). If the error mentions `127.0.0.1:8787` instead, the vendored plugin's 6767 default regressed.

### O5. Backend attributes the agent

```bash
"$HOME/Library/Application Support/Headroom/headroom/bin/rtk" proxy curl -s http://127.0.0.1:6767/stats | jq '.agent_usage.agents | map(.name // .agent // .id)' 2>/dev/null | grep -i opencode && echo PASS || echo "check .agent_usage shape by hand"
```

Expect: an `opencode` entry after O2/O3 traffic. (Key shape has drifted between backend versions - if the jq path misses, inspect `.agent_usage` by hand before calling FAIL.)

### O6. Dashboard shows blended rows (expected behavior, not a bug)

Open the dashboard savings chart tooltip. Expect the rows to read `Claude Code / OpenCode` and `Codex / OpenCode` while OpenCode is enabled. Backend rollups attribute by upstream provider only, so OpenCode savings blend into both rows by design until upstream grows an agent dimension. A dedicated OpenCode row appearing is a surprise; blended labels are the pass state.

### O7. Learn row scans OpenCode sessions

Learn view: an "OpenCode sessions" row must be present (connector enabled) and enabled (needs the `claude` CLI, or a signed-in `codex` CLI, for the analysis step). Hit "Scan now". Expect "Run succeeded" - O2/O3 above guarantee at least one session exists in `~/.local/share/opencode/`.

### O8. Disable restores the config

Toggle the OpenCode connector off in Settings, then:

```bash
jq -e '(.provider.anthropic.options.baseURL // "gone") != "http://127.0.0.1:6767/v1" and ((.plugin // []) | map(test("Headroom")) | any | not)' "$CFG" \
  && test ! -f ~/Library/Application\ Support/Headroom/opencode/entry.opencode.js \
  && echo PASS || echo FAIL
```

Expect: `PASS` - proxied baseURLs removed (or restored to the pre-Headroom value if you had one), plugin entry and plugin file gone. Re-enable afterwards and re-run O1.

## Cross-cutting

- **Banner badge**: the OpenCode logo badge shows gray = off, green = active, red only when the connector is enabled while the proxy is unreachable. Tooltip opens downward and instantly on hover.
- **Gate interplay**: with only OpenCode enabled, the Python backend must stay up (it is gate-exempt). `curl -s http://127.0.0.1:6767/livez` returns 200 with Claude and Codex both disabled.
- **Sentry**: the morning after, check the triage output for new error classes - misattributed connector errors (opencode traffic reported under codex fingerprints) would surface there first.

## When something fails

- `client=` missing or wrong in PERF lines: the intercept classification or X-Client stamp regressed (`proxy_intercept.rs`, `is_opencode`).
- Errors mentioning port 8787: vendored plugin default regressed (`entry.desktop.ts` wrapper, `src-tauri/resources/opencode/entry.opencode.js`).
- Config writes missing after enable: `configure_opencode_provider_block` in `client_adapters.rs`; it refuses configs managed by `headroom wrap opencode` - the connector error message says so explicitly.
