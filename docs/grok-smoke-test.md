# Grok Build smoke test

Companion to `beta-smoke-test.md` for the Grok Build connector (OpenCode has its own file, `opencode-smoke-test.md`). After installing a beta (`-rc.N`) build that carries it, paste this file into a Grok session - or into Claude Code - and ask the agent to run the checks. Each check has a single expected signal - if any fail, stop and investigate before promoting to stable.

The checks drive Grok with one-shot commands (`grok -p`), so they work from either agent's shell tool. One-shot runs complete before the command returns, so there is no "next turn" timing trap here (contrast check 7 in the main doc): the proxy log line exists the moment the command exits.

## Setup

1. Run the main `beta-smoke-test.md` setup (relaunch, tray, dashboard open).
2. Confirm the binary exists: `~/.grok/bin/grok --version`.
3. Enable the Grok Build connector in Settings if not already enabled.
4. `tail -0 -f ~/.headroom/logs/proxy.log` in a spare terminal helps, but every check below greps the log after the fact, so it is optional.

## Checks

### G1. Config block written and binary detected

```bash
grep -q '# >>> headroom:grok_build_proxy >>>' ~/.grok/config.toml 2>/dev/null \
  || grep -q 'base_url = "http://127.0.0.1:6867/v1"' ~/.grok/config.toml \
  && echo PASS || echo FAIL
```

Expect: `PASS`. Either the managed marker block, or - if you already had a `[model.grok-build]` table - your table's `base_url` redirected in place with a trailing `# was:` comment preserving the original.

### G2. Real completion through the compression pipeline

This is the one leg pre-release testing could not cover (routing and auth passthrough were verified against api.x.ai with a fake key; a real completion was not). Requires a real xAI key:

```bash
XAI_API_KEY=<real-key> ~/.grok/bin/grok -p "say hi" -m grok-build
grep 'client=grok_build' ~/.headroom/logs/proxy.log | tail -1
```

Expect: a normal answer, and a `PERF` line with `client=grok_build` and a `grok-*` model. Grok speaks the OpenAI chat format, so this traffic IS compressed - on a prompt with real context, `tok_saved` should be non-zero. If the answer errors with an OpenAI-worded message, the `x-headroom-base-url: https://api.x.ai` stamp regressed and traffic went to the wrong vendor - stop and investigate.

### G3. Savings land under the Codex row (expected behavior, not a bug)

After G2, dashboard savings for grok traffic appear inside the `Codex` (or `Codex / OpenCode`) row, not a `Grok Build` row. The backend cannot emit provider `xai` yet; the frontend mapping is ready and waiting. Do not file this against the RC.

### G4. Learn row scans Grok sessions

Learn view: a "Grok sessions" row must be present (connector enabled) and enabled (needs the `claude` CLI, or a signed-in `codex` CLI, for the analysis step). Hit "Scan now". Expect "Run succeeded" - G2 above guarantees at least one session exists in `~/.grok/sessions/`.

### G5. OAuth login mode (only if you use `grok login` instead of an API key)

Run one prompt in a fresh login shell (so `GROK_CLI_CHAT_PROXY_BASE_URL` from the managed shell block is exported) and confirm the same `client=grok_build` log attribution. This path is untested pre-release.

### G6. Disable restores the config

Toggle Grok Build off, then `cat ~/.grok/config.toml`: the managed block must be gone, and a redirected `base_url` must be back to its `# was:` value with the comment removed. Re-enable afterwards and re-run G1.

## Cross-cutting

- **Banner badge**: the Grok logo badge shows gray = off, green = active, red only when the connector is enabled while the proxy is unreachable. Tooltip opens downward and instantly on hover.
- **Gate interplay**: with only Grok Build enabled, the Python backend must stay up (it is gate-exempt). `curl -s http://127.0.0.1:6867/livez` returns 200 with Claude and Codex both disabled.
- **Sentry**: the morning after, check the triage output for new error classes - misattributed connector errors (grok traffic reported under codex fingerprints) would surface there first.

## When something fails

- `client=` missing or wrong in PERF lines: the intercept classification or X-Client stamp regressed (`proxy_intercept.rs`, `is_grok`).
- Errors worded by OpenAI: `x-headroom-base-url` stamp missing; traffic misrouted to api.openai.com.
- Config writes missing after enable: the grok apply arm in `client_adapters.rs`.
