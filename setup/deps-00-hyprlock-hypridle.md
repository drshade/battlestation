# hyprlock + hypridle (suspend/idle locker)

hyprlock/hypridle own the **suspend/idle** lock instead of Noctalia's in-shell
locker: a separate locker process can't share the shell's fate — an in-shell
lock whose shell crashes leaves the compositor holding an `ext-session-lock`
with nothing rendering the password prompt (see AGENTS.md gotchas).

```sh
sudo pacman -S hyprlock hypridle
```

Config is stowed (`hypr/.config/hypr/{hyprlock,hypridle}.conf`) and hypridle is
launched from `autostart.lua`; nothing else to enable. hypridle answers logind's
Lock / PrepareForSleep signals; its one timeout `listener` only reports
presence to `bsctl presence` (for the agent desk — contract in
`ctl/src/lib.rs`) and changes no idle behaviour.

Two `settings.json` values route everything to hyprlock and must stay in
lockstep: `general.lockOnSuspend` is `false` (Noctalia doesn't lock on sleep),
and the session menu's lock entry
(`sessionMenu.powerOptions[action=lock].command`) is `loginctl lock-session` —
`CompositorService.lock()` runs that and returns before its own `WlSessionLock`,
so `Hyper+L` → lock also lands on hyprlock. Noctalia's in-shell locker is thus
unreached in normal use; `restart_crashed_lock.sh` survives only for the case it
is deliberately re-enabled.
