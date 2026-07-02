# KDE Connect (phone ↔ desktop)

```sh
sudo pacman -S kdeconnect
```

The package ships an XDG autostart entry, but under uwsm/Hyprland the
systemd-xdg-autostart generator marks it `ConditionResult=no`, so the daemon
never launches. `stow/home/kdeconnect` provides a dedicated user unit instead,
enabled by its stow-managed `graphical-session.target.wants/` symlink (see
AGENTS.md on repo-owned user services) — no enable step. After the first stow,
start it without waiting for a re-login:

```sh
systemctl --user daemon-reload && systemctl --user start kdeconnect.service
```

Open the firewall — ufw drops the discovery/transfer ports (1714:1764 tcp+udp)
otherwise, so the desktop never appears in the mobile app. The package ships a
ufw app profile, so:

```sh
sudo ufw allow KDEConnect
```

ufw owns `/etc/ufw/user{,6}.rules`, so this isn't stowable — it's a one-off that
persists across reboots once applied. Pair from the phone, then `kdeconnect-cli -l`.
