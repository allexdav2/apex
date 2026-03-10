#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
AGENTS_SRC="$REPO_ROOT/agents/agents"
COMMANDS_SRC="$REPO_ROOT/agents/commands"
AGENTS_DEST="$REPO_ROOT/.claude/agents"
COMMANDS_DEST="$REPO_ROOT/.claude/commands"

mkdir -p "$AGENTS_DEST" "$COMMANDS_DEST"

installed=0

for f in "$AGENTS_SRC"/*.md; do
    name="$(basename "$f")"
    cp "$f" "$AGENTS_DEST/$name"
    echo "  agent:   $name"
    ((installed++))
done

for f in "$COMMANDS_SRC"/*.md; do
    name="$(basename "$f")"
    cp "$f" "$COMMANDS_DEST/$name"
    echo "  command: $name"
    ((installed++))
done

echo ""
echo "Installed $installed agents/commands into .claude/"
echo ""
echo "Slash commands available:"
for f in "$COMMANDS_SRC"/*.md; do
    name="$(basename "$f" .md)"
    echo "  /$name"
done
