# CachyOS ships its fish defaults here; absent on other systems.
test -f /usr/share/cachyos-fish-config/cachyos-config.fish
and source /usr/share/cachyos-fish-config/cachyos-config.fish

function fish_greeting
    fastfetch \
        --pipe false \
        --kitty "$HOME/.config/fish/battlestation-logo.png" \
        --logo-preserve-aspect-ratio \
        --logo-width 28 \
        --logo-padding 2 \
        --logo-padding-top 6
end
export PATH="$HOME/.local/bin:$PATH"

set -q GHCUP_INSTALL_BASE_PREFIX[1]; or set GHCUP_INSTALL_BASE_PREFIX $HOME ; set -gx PATH $HOME/.cabal/bin $PATH $HOME/.ghcup/bin # ghcup-env
