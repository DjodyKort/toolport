# Managed by `toolportctl context` — do not edit by hand.
# Source from ~/.zshrc AFTER cf's shell-wrapper.sh and compression-shims.zsh:
#   source ~/.config/mcpm/context-shims.zsh

claude-cp() { CLAUDE_CONFIG_DIR="<HOME>/.config/mcpm/claude-profiles/cp" command claude "$@"; }
claude-inh() { CLAUDE_CONFIG_DIR="<HOME>/.config/mcpm/claude-profiles/inh" command claude "$@"; }
