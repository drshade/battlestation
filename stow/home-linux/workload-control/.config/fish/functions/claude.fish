function claude --description 'Run Claude Code in the agent workload slice' --wraps=claude
    systemd-run --user --scope \
        --slice=agents.slice \
        --nice=5 \
        --quiet --collect \
        -- claude $argv
end
