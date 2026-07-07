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
Lock / PrepareForSleep signals. It has two timeout `listener`s: the first reports
presence to `bsctl presence` (for the agent desk — contract in `ctl/src/lib.rs`),
the second blanks the displays 10s after the session locks. There is still no
idle-lock and no screen-off during normal use.

`after_sleep_cmd` runs `displays-on.sh`, which DPMS-on's every enabled output —
but a plain DPMS toggle can only act on *enabled* outputs. Suspend docked-and-shut,
unplug the external, and resume with zero enabled outputs (internal panel disabled
by clamshell, external gone, and the lid-open edge that would re-enable eDP is lost
across sleep) and the screen stays black with nothing to toggle. `displays-on.sh`
detects that no-output case and escalates itself to `reset` (reload re-enables
outputs, `clamshell.sh auto` reconciles the real lid state), so resume self-heals.

## Blank-on-lock

hyprlock only paints the lock surface (the clock); it does no DPMS. The screen
goes black via a second hypridle `listener` (`timeout = 10`) whose commands are
both gated on `pidof hyprlock`, so they are a no-op while unlocked and only fire
once hyprlock owns the screen. `on-timeout` blanks the outputs; `on-resume`
(any input) wakes them back to the lock clock, where you type your password.

That listener sets `ignore_inhibit = true`, and it must: hyprlock holds an idle
inhibitor whenever it is the active locker, so under hypridle's default
(inhibitors respected) the timeout never fires — precisely while locked, the one
time we want it to. Ignoring the inhibitor is safe because the *action* stays
gated on `pidof hyprlock`: the timeout may also fire during an unlocked video,
but `displays-off.sh` only runs when locked.

DPMS is toggle-only on the Lua build (it ignores the on/off arg — see AGENTS.md
gotchas), so neither direction uses `hyprctl dispatch dpms off/on`. Both go
through matched scripts that read each output's `dpmsStatus` and toggle only the
outputs on the wrong side: `scripts/displays-on.sh` (wake, idempotent) and its
mirror `scripts/displays-off.sh` (blank). To change the delay, edit the second
listener's `timeout`.

Two `settings.json` values route everything to hyprlock and must stay in
lockstep: `general.lockOnSuspend` is `false` (Noctalia doesn't lock on sleep),
and the session menu's lock entry
(`sessionMenu.powerOptions[action=lock].command`) is `loginctl lock-session` —
`CompositorService.lock()` runs that and returns before its own `WlSessionLock`,
so `Hyper+L` → lock also lands on hyprlock. Noctalia's in-shell locker is thus
unreached in normal use; `restart_crashed_lock.sh` survives only for the case it
is deliberately re-enabled.
