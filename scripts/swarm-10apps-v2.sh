#!/usr/bin/env bash
# 10-app native swarm test v2 — per-app 90s timeout, continue-on-fail,
# simpler codex objectives. ruflo commands run AFTER the swarm build.
set -uo pipefail  # no -e: continue on failure

RUFLO="${1:-/mnt/datadisk/dev/ruflo_rust/target/release/ruflo}"
ROOT="/tmp/ruflo-swarm-test2"
mkdir -p "$ROOT"
PASS=0; FAIL=0

build_and_test() {
  local name="$1" lang="$2" desc="$3"
  local dir="$ROOT/$name"
  rm -rf "$dir"; mkdir -p "$dir"; cd "$dir"
  echo "=== [$name] ==="
  "$RUFLO" init >/dev/null 2>&1
  "$RUFLO" swarm init >/dev/null 2>&1
  # Swarm: codex worker builds the app (90s timeout)
  timeout 90 "$RUFLO" swarm start --objective "$desc" --workers 1 --agent codex 2>&1 | grep -E "Swarm complete|worker 1" | head -2
  local rc=$?
  # ruflo commands exercised (fast, local — no LLM)
  "$RUFLO" memory store --key "${name}-built" --value "yes" --path "$dir/m.db" >/dev/null 2>&1 && echo "  memory store OK"
  "$RUFLO" embeddings generate -t "$desc" 2>/dev/null | grep -q "Dimensions" && echo "  embeddings OK"
  "$RUFLO" security scan -t "$dir" --depth quick 2>/dev/null | grep -q "Total" && echo "  security scan OK"
  "$RUFLO" hooks route -t "$desc" 2>/dev/null | grep -q "Agent" && echo "  hooks route OK"
  if [ $rc -eq 0 ]; then PASS=$((PASS+1)); echo "  [$name] PASS"; else FAIL=$((FAIL+1)); echo "  [$name] FAIL(rc=$rc)"; fi
  echo
}

build_and_test "01-todo-cli"     rust   "Create a Rust CLI todo app (main.rs with add/list/done)"
build_and_test "02-url-short"    python "Create a Python url shortener (app.py with Flask)"
build_and_test "03-md-blog"      python "Create a Python markdown to HTML converter (gen.py)"
build_and_test "04-weather"      node   "Create a Node.js weather fetcher (index.js using open-meteo API)"
build_and_test "05-echo-bot"     python "Create a Python echo bot (bot.py reads stdin echoes back)"
build_and_test "06-encryptor"    rust   "Create a Rust XOR file encryptor (main.rs encrypt/decrypt)"
build_and_test "07-recipe"       python "Create a Python recipe finder (find.py searches JSON)"
build_and_test "08-pomodoro"     rust   "Create a Rust pomodoro timer (main.rs 25min countdown)"
build_and_test "09-expense"      python "Create a Python expense tracker (track.py add/list/sum CSV)"
build_and_test "10-snippet"      rust   "Create a Rust snippet manager (main.rs store/list JSON)"

echo "=============================="
echo "RESULTS: $PASS passed, $FAIL failed (out of 10)"
