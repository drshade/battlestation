# KDE Connect (phone ↔ desktop)

```sh
sudo pacman -S kdeconnect
```

The package ships an XDG autostart entry, but under uwsm/Hyprland the
systemd-xdg-autostart generator marks it `ConditionResult=no`, so the daemon
never launches. `stow/home/kdeconnect` provides a dedicated user unit instead —
stow it, then:

```sh
systemctl --user enable --now kdeconnect.service
```

Open the firewall — ufw drops the discovery/transfer ports (1714:1764 tcp+udp)
otherwise, so the desktop never appears in the mobile app. The package ships a
ufw app profile, so:

```sh
sudo ufw allow KDEConnect
```

ufw owns `/etc/ufw/user{,6}.rules`, so this isn't stowable — it's a one-off that
persists across reboots once applied. Pair from the phone, then `kdeconnect-cli -l`.
