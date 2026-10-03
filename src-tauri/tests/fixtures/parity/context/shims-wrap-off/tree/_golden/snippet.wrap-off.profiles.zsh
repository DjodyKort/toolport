# Managed by `mcpm context` — do not edit by hand.
# Source from ~/.zshrc AFTER cf's shell-wrapper.sh and compression-shims.zsh:
#   source ~/.config/mcpm/context-shims.zsh

claude-bare() { CLAUDE_CONFIG_DIR="<HOME>/.config/mcpm/claude-profiles/bare" command claude "$@"; }
claude-work() { CLAUDE_CONFIG_DIR="<HOME>/.config/mcpm/claude-profiles/work" command claude "$@"; }
