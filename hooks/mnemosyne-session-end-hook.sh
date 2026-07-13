#!/usr/bin/env bash
# Auto-save: sweep Claude Code transcripts into the palace in the
# background. Silent + fast: the sweep is idempotent (keyed content
# fingerprints), so re-running never duplicates drawers, and nothing is
# printed into the chat window.
command -v mnemosyne >/dev/null 2>&1 || exit 0
nohup mnemosyne sweep "$HOME/.claude/projects" --wing claude-code >/dev/null 2>&1 &
exit 0
