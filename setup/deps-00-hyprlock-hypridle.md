# hyprlock + hypridle (suspend/idle locker)

Moves the **suspend/idle** lock surface off the Noctalia shell onto a dedicated
locker. Noctalia drew the lock itself, so a shell crash on resume left the
compositor holding an `ext-session-lock` with nothing rendering the password
prompt — a black screen indistinguishable from a failed resume (and a wrong
guess at that point — a short power-key tap — cleanly powers the machine off,
losing the session). A separate locker process can't share that fate.

```sh
sudo pacman -S hyprlock hypridle
```

Config is stowed (`hypr/.config/hypr/{hyprlock,hypridle}.conf`) and hypridle is
launched from `autostart.lua`; nothing else to enable. hypridle answers logind's
Lock / PrepareForSleep signals, so it needs no timeout `listener` blocks — idle
behaviour is unchanged.

Two `settings.json` values route everything to hyprlock and must stay in
lockstep: `general.lockOnSuspend` is `false` (Noctalia doesn't lock on sleep),
and the session menu's lock entry
(`sessionMenu.powerOptions[action=lock].command`) is `loginctl lock-session` —
`CompositorService.lock()` runs that and returns before its own `WlSessionLock`,
so `Hyper+L` → lock also lands on hyprlock. Noctalia's in-shell locker is thus
unreached in normal use; `restart_crashed_lock.sh` survives only for the case it
is deliberately re-enabled.
