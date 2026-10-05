#!/bin/sh
# Stand-in for `claude plugin ...`: records every call's argv and stdin next to this script and
# answers `plugin configure <id> --json` from configure.json. `plugin list` fails so that the
# caller reads the plugin files.
d="$(dirname "$0")/stub-data"
mkdir -p "$d"
n=$(( $(cat "$d/count" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$d/count"
printf '%s\n' "$*" >> "$d/argv.log"
case "$*" in
  "plugin configure "*" --json") cat "$d/configure.json" ;;
  "plugin configure "*" --values-stdin") cat > "$d/stdin.$n.json"; echo configured ;;
  "plugin disable "*" --scope user"|"plugin enable "*" --scope user")
    if [ -f "$d/fail" ]; then echo "plugin is managed by policy" >&2; exit 4; fi
    echo ok ;;
  *) echo "unexpected: $*" >&2; exit 3 ;;
esac
