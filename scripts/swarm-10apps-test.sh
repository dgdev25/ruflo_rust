#!/usr/bin/env bash
# 10-app native swarm test — each app is a swarm objective that exercises
# different ruflo-rust commands. Maximizes command coverage.
set -euo pipefail

RUFLO="${1:-/mnt/datadisk/dev/ruflo_rust/target/release/ruflo}"
ROOT="/tmp/ruflo-swarm-test"
mkdir -p "$ROOT"
PASS=0; FAIL=0

run_app() {
  local name="$1" obj="$2" agent="${3:-codex}" workers="${4:-1}"
  local dir="$ROOT/$name"
  rm -rf "$dir"; mkdir -p "$dir"
  echo "=== [$name] starting (agent=$agent workers=$workers) ==="
  cd "$dir"
  "$RUFLO" init >/dev/null 2>&1
  "$RUFLO" swarm init >/dev/null 2>&1
  timeout 180 "$RUFLO" swarm start --objective "$obj" --workers "$workers" --agent "$agent" 2>&1 | tail -6
  local rc=$?
  if [ $rc -eq 0 ]; then echo "[$name] PASS"; PASS=$((PASS+1)); else echo "[$name] FAIL (rc=$rc)"; FAIL=$((FAIL+1)); fi
  echo
}

# Each app exercises different ruflo commands in its objective prompt.
run_app "01-todo-cli" \
  "Build a minimal Rust CLI todo app (main.rs: add/list/done subcommands). Then run: ruflo memory store --key todo-feature --value done --path mem.db; ruflo security scan -t . --depth quick; ruflo embeddings generate -t 'todo app'." \
  codex 1

run_app "02-url-shortener" \
  "Create a Python url shortener (app.py: Flask, /shorten, /r/<id>). Then run: ruflo memory store --key urls --value working --path mem.db; ruflo hooks route -t 'build a url shortener'; ruflo embeddings compare --text1 'url shortener' --text2 'link service'." \
  codex 1

run_app "03-markdown-blog" \
  "Create a static markdown blog generator (gen.py: reads .md files, outputs .html). Then run: ruflo neural train -p coordination -e 3; ruflo security scan -t . --depth quick; ruflo memory store --key blog-entry --value hello --path mem.db." \
  codex 1

run_app "04-weather-dash" \
  "Create a Node.js weather dashboard (index.js: fetches open-meteo, prints temp). Then run: ruflo embeddings generate -t 'weather dashboard'; ruflo hooks pre-edit --file index.js; ruflo memory store --key weather --value sunny --path mem.db." \
  codex 1

run_app "05-chat-bot" \
  "Create a Python echo chatbot (bot.py: reads stdin, echoes with prefix). Then run: ruflo memory store --key chat-log --value hello --path mem.db; ruflo memory retrieve --key chat-log --path mem.db; ruflo embeddings compare --text1 'hello' --text2 'hi'." \
  codex 1

run_app "06-file-encryptor" \
  "Create a Rust file encryptor (main.rs: XOR cipher, encrypt/decrypt subcommands). Then run: ruflo security secrets -p .; ruflo security scan -t . --depth standard; ruflo memory store --key crypto --value aes --path mem.db." \
  codex 1

run_app "07-recipe-finder" \
  "Create a Python recipe finder (find.py: searches a JSON recipe DB by ingredient). Then run: ruflo memory store --key recipe --value pasta --path mem.db; ruflo memory search -q 'pasta' --path mem.db; ruflo embeddings generate -t 'recipe finder'." \
  codex 1

run_app "08-pomodoro" \
  "Create a Rust pomodoro timer (main.rs: 25min focus, 5min break, CLI countdown). Then run: ruflo hooks pre-task --description 'build pomodoro'; ruflo hooks metrics; ruflo neural status." \
  codex 1

run_app "09-expense-tracker" \
  "Create a Python expense tracker (track.py: add/list/sum expenses in CSV). Then run: ruflo memory store --key expense --value coffee-3.50 --path mem.db; ruflo daemon budget show; ruflo security scan -t . --depth quick." \
  codex 1

run_app "10-snippet-manager" \
  "Create a Rust snippet manager (main.rs: store/list/search code snippets in JSON). Then run: ruflo memory store --key snippet --value 'fn hello(){}' --path mem.db; ruflo memory retrieve --key snippet --path mem.db; ruflo embeddings compare --text1 'snippet manager' --text2 'code library'." \
  codex 1

echo "=============================="
echo "RESULTS: $PASS passed, $FAIL failed (out of 10)"
echo "Apps in: $ROOT"
