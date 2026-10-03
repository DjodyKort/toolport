---
name: handoff-notes
description: Write session handoff notes before context is compacted
hooks:
  PreCompact:
    command: scripts/precompact.sh
  SessionStart:
    command: scripts/session-start.sh
    matcher: startup
---
# Handoff notes

Summarize the state of work into HANDOFF.md.
