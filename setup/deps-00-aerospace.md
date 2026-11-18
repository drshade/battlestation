# AeroSpace (macOS window manager)

On macOS, AeroSpace provides the workspaces bsctl drives (`ctl/src/lib.rs`,
"Window managers"). Its config is stowed from `stow/home-darwin/aerospace`;
the cask is declared in `packages/brew-cask.txt`.

macOS grants the Accessibility permission only by hand, so the first launch
is out of band:

```sh
open -a AeroSpace   # then System Settings → Privacy & Security → Accessibility → allow AeroSpace
aerospace list-workspaces --all   # should print 1..10
```

The config declares workspaces 1–10 as persistent because bsctl only sees
workspaces whose name is an integer. Leave them numeric; name them through
bsctl instead (`bsctl ws name set --bs-id N --name api`), which is what the
board and the keybinds read. `start-at-login = true` in the config handles
later logins.

bsctl follows AeroSpace's event stream (`aerospace subscribe`); a stream
started before AeroSpace was running reconnects on its own within ~10s.
