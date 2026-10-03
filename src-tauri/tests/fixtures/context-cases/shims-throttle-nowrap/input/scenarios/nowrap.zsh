set +e
bin=$HOME/stubbin; mkdir -p $bin
printf '#!/bin/sh\necho "toolportctl $*" >> $HOME/stub.log\n' > $bin/toolportctl
printf '#!/bin/sh\necho "claude $* CLAUDE_CONFIG_DIR=${CLAUDE_CONFIG_DIR:-<unset>}" >> $HOME/stub.log\n' > $bin/claude
chmod +x $bin/*
export PATH=$bin:/usr/bin:/bin
source $HOME/.config/mcpm/context-shims.zsh
claude-work x
echo "claude-work defined: $(( $+functions[claude-work] )); claude fn defined: $(( $+functions[claude] )); presync defined: $(( $+functions[mcpm_context_presync] ))"
cat $HOME/stub.log
