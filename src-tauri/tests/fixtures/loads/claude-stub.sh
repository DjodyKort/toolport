#!/bin/sh
# A stand-in for `claude` that answers what `context measure` asks, with recorded-shape
# stream-json events (synthetic values: nothing here came from a real session).
#
#   claude --version                      prints $STUB_CLAUDE_VERSION (or the content of
#                                         $STUB_CLAUDE_VERSION_FILE), default 2.1.289
#   claude -p ... [--settings <file>]     prints stream-as-is.jsonl, or stream-plugin-off.jsonl
#                                         when the settings file turns a plugin off, or
#                                         stream-skills-off.jsonl when it hides skills
#
# $STUB_CLAUDE_LOG, when set, receives one line per call: `version` or
# `request cwd=<physical cwd> model=<model> settings=<yes|no>`.

here=$(cd "$(dirname "$0")" && pwd)

log() {
  if [ -n "${STUB_CLAUDE_LOG:-}" ]; then
    printf '%s\n' "$*" >> "$STUB_CLAUDE_LOG"
  fi
}

if [ "${1:-}" = "--version" ]; then
  log version
  version="${STUB_CLAUDE_VERSION:-2.1.289}"
  if [ -n "${STUB_CLAUDE_VERSION_FILE:-}" ] && [ -f "$STUB_CLAUDE_VERSION_FILE" ]; then
    version=$(cat "$STUB_CLAUDE_VERSION_FILE")
  fi
  echo "$version (Claude Code)"
  exit 0
fi

settings=""
model=""
while [ $# -gt 0 ]; do
  case "$1" in
    --settings) settings="${2:-}"; shift ;;
    --model) model="${2:-}"; shift ;;
  esac
  shift
done

if [ -n "$settings" ]; then
  used=yes
else
  used=no
fi
log "request cwd=$(pwd -P) model=$model settings=$used"

stream=as-is
if [ -n "$settings" ] && [ -f "$settings" ]; then
  if grep -q '"enabledPlugins"' "$settings"; then
    stream=plugin-off
  elif grep -q '"skillOverrides"' "$settings"; then
    stream=skills-off
  fi
fi
cat "$here/stream-$stream.jsonl"
