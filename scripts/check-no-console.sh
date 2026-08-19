#!/usr/bin/env bash
# Every child process must spawn through crate::proc::command so Windows users
# don't get a console window per pip/reg/powershell invocation. See src/proc.rs.
set -euo pipefail
cd "$(dirname "$0")/.."
hits=$(grep -rn 'Command::new(' src-tauri/src --include='*.rs' | grep -v '^src-tauri/src/proc.rs:' || true)
if [ -n "$hits" ]; then
  echo "Bare Command::new() spawns a visible console on Windows."
  echo "Use crate::proc::command() instead:"
  echo "$hits"
  exit 1
fi
echo "check-no-console: all spawns route through crate::proc::command"
