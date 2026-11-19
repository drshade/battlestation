# launchd agents (macOS)

Repo-owned user agents live in `stow/home-darwin/*/Library/LaunchAgents/`.
launchd loads everything in `~/Library/LaunchAgents` at login, so stowing is
enabling; the only out-of-band step is starting one without logging out:

```sh
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/<label>.plist
launchctl print gui/$(id -u)/<label> | grep -E 'state|last exit'   # verify
```

After editing a stowed plist, `launchctl bootout gui/$(id -u)/<label>` then
bootstrap again (launchd caches the plist it loaded).
