# CLAUDE.md - headroom-desktop

This file holds only what the model cannot derive: project invariants, environment
quirks, and concrete commands. Generic coding-style coaching (write simply, read
before editing, state the bug and stop) was removed on 2026-08-03 — modern models do
it unprompted and the harness enforces the rest. Do not re-add it.

## Testing Rules
- After any code change, run the relevant tests/checks before declaring the task done. Do not ask the user to verify.
- Rust changes: `cargo test --manifest-path src-tauri/Cargo.toml --lib <filter>` for the affected module, plus `cargo check --manifest-path src-tauri/Cargo.toml` if the change crosses module boundaries.
- Frontend changes: `npx tsc --noEmit` and any relevant Vitest suite. For visual changes, see Styling Rules.
- If a test cannot be run in this environment, say so explicitly rather than skipping silently.

## Persistence Rules
Most stability bugs in this codebase's history were violations of one of these five. Follow them for any new code; treat violations found in existing code as bugs.
- Anything persisted uses `client_adapters::atomic_write` (tmp+rename), never plain `fs::write`. Crash mid-write must not truncate state.
- Anything versioned/deserialized carries `#[serde(default)]` (container-level where possible). One added required field must not wipe a user's history. On parse/schema failure: back the file up and log, never silently overwrite; salvage format-agnostic fields where possible.
- Anything appended (logs, JSONL) has a size cap or rotation from day one.
- Never kill a pid resolved from a port without verifying its identity (argv/process name) first.
- Day/hour bucket keys must state their timezone. User-facing "days" are local (`local_day_key`); if a source is UTC-bucketed (backend rollups), key it by its UTC date and say so - never relabel one as the other.

## Formatting
- No em dashes, smart quotes, or decorative Unicode. Plain hyphens and straight quotes, so output stays copy-paste safe. Accented letters and CJK are fine when the content needs them.

## Styling Rules
- Never hardcode colors in component CSS. Use the semantic tokens defined in `:root` in `src/styles.css` (`--surface-*`, `--text-*`, `--border-*`, `--fill-*`, `--accent*`, `--warning*`, `--danger*`, `--chip-*`).
- If a needed color does not exist as a token, add it to both the `:root` block and the `@media (prefers-color-scheme: dark)` override — do not inline a hex/rgba in a component rule.
- Exceptions: pure `#fff`/`#000`, brand gradients, and launcher/splash-only one-offs that are intentionally theme-invariant. Comment the exception inline.
- When adding or modifying a component, visually verify both light and dark mode before declaring done. If dark mode cannot be tested, say so explicitly.
- Run `npm run check:colors` (or `./scripts/check-colors.sh`) on any CSS you touch. It flags raw hex/rgba in component rules. Migrate any new offenders to tokens before committing. Existing offenders are the Stage 4 migration backlog — don't add to them.
