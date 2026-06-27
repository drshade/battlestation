-- Keep workspaces 1..N alive even when empty, so they stay on the bar.
-- Hyprland destroys empty non-persistent workspaces, which makes them disappear.
for i = 1, 9 do
    hl.workspace_rule({ workspace = tostring(i), persistent = true })
end
