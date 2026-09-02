#!/usr/bin/env bash
# Research Core — debug mode launcher.
#
# Usage:  ./debug.sh
#
# Runs the app with Vite live-reload + Rust debug build + WebView devtools.
# - The WebView opens automatically; open devtools via the "Debug" menu
#   (Toggle Developer Tools) or Cmd+Alt+I.
# - Frontend edits hot-reload instantly; Rust edits trigger a recompile.
# - Boot diagnostics are written to /tmp/rc-diag.log .
set -euo pipefail
cd "$(dirname "$0")"
echo "» Launching Research Core in debug mode (cargo tauri dev)…"
echo "» WebView devtools: Debug menu → Toggle Developer Tools (or Cmd+Alt+I)"
echo "» Diagnostics log:  /tmp/rc-diag.log"
npx tauri dev
