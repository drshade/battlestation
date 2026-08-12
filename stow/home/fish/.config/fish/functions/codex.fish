function codex --description 'Run Codex in the agent workload slice' --wraps=codex
    systemd-run --user --scope \
        --slice=agents.slice \
        --nice=5 \
        --quiet --collect \
        -- codex $argv
end
