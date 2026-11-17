# Managed by `mcpm compression` — do not edit by hand.
# Source from ~/.zshrc:  source ~/.config/mcpm/compression-shims.zsh
# (replaces the old hand-maintained ~/.config/headroom-aliases.zsh)
hrclaude() { mcpm compression run -- "$@"; }   # launch claude under the per-dir policy
hrup()     { mcpm compression proxy up; }        # start the active-preset proxy
hrdown()   { mcpm compression proxy down; }      # stop it
hrrestart(){ mcpm compression proxy restart; }   # restart (apply a mode change)
hrstat()   { mcpm compression status; }
hrupdate() { mcpm compression update --latest --accept; }  # pin newest headroom + re-snapshot presets
hrperf()   { headroom perf "$@"; }              # savings report (headroom passthrough)
hrdash()   { open "http://127.0.0.1:8787/dashboard" 2>/dev/null || headroom perf; }
