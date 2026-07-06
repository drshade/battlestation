# zsh + oh-my-zsh

The login shell is zsh with oh-my-zsh; `stow/home/zsh` provides `~/.zshrc`.
The zsh/fzf packages are declared in `packages/pacman.txt`. Out-of-band steps:

oh-my-zsh installs itself into `~/.oh-my-zsh` (framework code, not config —
it self-updates, so it is not tracked here):

```sh
sh -c "$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)" "" --unattended --keep-zshrc
```

The `.zshrc` plugin list expects these custom plugins (not part of oh-my-zsh
core; cloned, not tracked, same reasoning as the framework):

```sh
git clone https://github.com/zsh-users/zsh-autosuggestions ~/.oh-my-zsh/custom/plugins/zsh-autosuggestions
git clone https://github.com/zsh-users/zsh-syntax-highlighting ~/.oh-my-zsh/custom/plugins/zsh-syntax-highlighting
git clone https://github.com/unixorn/fzf-zsh-plugin ~/.oh-my-zsh/custom/plugins/fzf-zsh-plugin
```

Make it the login shell (kitty runs the login shell, so this is the whole
switch; the fish package stays stowed and works if launched explicitly):

```sh
chsh -s "$(which zsh)"
```
