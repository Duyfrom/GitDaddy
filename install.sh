#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

./build_app.sh --install

if [ -d "/Applications/GitDaddy.app" ]; then
    open "/Applications/GitDaddy.app"
elif [ -d "$HOME/Applications/GitDaddy.app" ]; then
    open "$HOME/Applications/GitDaddy.app"
else
    open "$SCRIPT_DIR/GitDaddy.app"
fi
