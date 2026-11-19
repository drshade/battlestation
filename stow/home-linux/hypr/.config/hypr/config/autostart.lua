-- Auto-start config
-- if you dont use UWSM add your auto start programs here, otherwise use XDG autostart https://wiki.archlinux.org/title/XDG_Autostart

hl.on("hyprland.start", function ()
    hl.exec_cmd("dbus-update-activation-environment --systemd --all")
    hl.exec_cmd("qs -c noctalia-shell")
    hl.exec_cmd("hypridle") -- locks via hyprlock on suspend/idle (see hypridle.conf)
    -- Logging in IS presence: seed the state that hypridle's listeners only
    -- update on idle/resume transitions (contract in ctl/src/lib.rs).
    hl.exec_cmd("$HOME/.local/bin/bsctl presence set active")
    hl.exec_cmd("xhost +SI:localuser:root")
end)
