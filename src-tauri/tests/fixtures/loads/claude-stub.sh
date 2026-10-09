#!/bin/sh
# A stand-in for `claude` that answers what `context measure` asks, with recorded-shape
# stream-json events (synthetic values: nothing here came from a real session).
#
#   claude --version                      prints $STUB_CLAUDE_VERSION (or the content of
#                                         $STUB_CLAUDE_VERSION_FILE), default 2.1.289
#   claude -p ... [--settings <file>]     writes the InstructionsLoaded events of
#                                         $STUB_CLAUDE_INSTRUCTIONS to the hook log of the
#                                         settings file, then prints stream-as-is.jsonl, or stream-plugin-off.jsonl
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

used=no
if [ -n "$settings" ] && [ -f "$settings" ] && grep -q -e '"enabledPlugins"' -e '"skillOverrides"' "$settings"; then
  used=yes
fi
log "request cwd=$(pwd -P) model=$model settings=$used"

# The InstructionsLoaded hook of the as-is request: one event per path of
# $STUB_CLAUDE_INSTRUCTIONS (colon separated), appended to the log the hook command names.
if [ -n "$settings" ] && [ -f "$settings" ] && [ -n "${STUB_CLAUDE_INSTRUCTIONS:-}" ]; then
  hook_log=$(sed -n "s/.*cat >> '\([^']*\)'.*/\1/p" "$settings" | head -1)
  if [ -n "$hook_log" ]; then
    printf '%s\n' "$STUB_CLAUDE_INSTRUCTIONS" | tr ':' '\n' | while IFS= read -r file; do
      [ -n "$file" ] && printf '{"hook_event_name":"InstructionsLoaded","file_path":"%s","memory_type":"User","load_reason":"session_start"}\n' "$file" >> "$hook_log"
    done
  fi
fi

stream=as-is
if [ -n "$settings" ] && [ -f "$settings" ]; then
  if grep -q '"enabledPlugins"' "$settings"; then
    stream=plugin-off
  elif grep -q '"skillOverrides"' "$settings"; then
    stream=skills-off
  fi
fi
cat "$here/stream-$stream.jsonl"
