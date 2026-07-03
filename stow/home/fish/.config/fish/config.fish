source /usr/share/cachyos-fish-config/cachyos-config.fish

function fish_greeting
    fastfetch \
        --pipe false \
        --kitty "$HOME/.config/fish/battlestation-logo.png" \
        --logo-preserve-aspect-ratio \
        --logo-width 40 \
        --logo-padding 2 \
        --logo-padding-top 4
end
export PATH="$HOME/.local/bin:$PATH"

set -q GHCUP_INSTALL_BASE_PREFIX[1]; or set GHCUP_INSTALL_BASE_PREFIX $HOME ; set -gx PATH $HOME/.cabal/bin $PATH /home/tom/.ghcup/bin # ghcup-env
