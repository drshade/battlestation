# Links open in a browser window on the current workspace

**Problem:** Firefox opens a handed-over URL in its most recently focused
window, wherever that is. With a Firefox window on most workspaces, a link
clicked in a terminal appears in some other workspace's window.

**Fix:** `browser-here.sh` (hypr package, `scripts/`) is the default browser
via `browser-here.desktop` (hypr package, `.local/share/applications/`). It
focuses the Firefox window on the active workspace and hands over the URL, or
opens a new window here if there is none. HYPER+B runs the same script.

The default-browser choice itself is user state (`~/.config/mimeapps.list`),
not stowed, so register it once per machine after stowing:

```sh
xdg-settings set default-web-browser browser-here.desktop
xdg-mime default browser-here.desktop x-scheme-handler/http x-scheme-handler/https text/html
xdg-settings get default-web-browser   # -> browser-here.desktop
```

To use another browser, change `browser`/`class` at the top of the script.

`xdg-open` consults mimeapps first, so the above is enough for it. Tools that
read `$BROWSER` directly bypass that, so `~/.config/uwsm/env` (untracked
session env, sourced at login) exports `BROWSER` as the script's path rather
than `firefox`. Takes effect at next login.
