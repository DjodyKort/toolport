"""Runs mcpm's `sync` command group inside the mcpm venv for gen.sh; only the tap, search and
install commands are exercised."""

from mcpm_sync.cli import sync_group

sync_group()
