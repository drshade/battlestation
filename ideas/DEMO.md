
# Why

My previous desktop was high-friction for working across multiple projects in parallel
 - combination of many windows all jumbled together
 - using tools like obsidian / notion for trying to manage information - but this was disconnected from my actual workspace where the work was actually happening

Optimise my workstation for the way I actually work
 - lots of parallel things happening, overlapping in space & time - some boring, some exciting, some quick, some slow
 - want to see my list of things together with the space that i work on them
 - when using agents, i dont want to watch them work, but easily see when they need me

# Outgrew OSX

 - No way to easily organise and name my workspaces
 - Lots of cmd-tabbing through terminal windows
 - Lots of open browser tabs
 - OSX is really optimised for a single task (imho)

# Took the leap to try out linux desktop (again...)

| thing                       | what                  | and                              |
|-----------------------------|-----------------------|----------------------------------|
| Linux distro                | cachyos               | its an arch btw                  |
| Display Manager             | wayland               | newer x11                        |
| Compositor (window manager) | hyprland              | minimal, super scriptable        |
| Desktop Shell               | quickshell            | desktop framework                |
| Desktop Widgets             | noctalia              | bunch of components, themes, etc |
| Terminal                    | kitty                 | super scriptable                 |
| Agents + Deck               | battlestation + bsctl | my custom workflow               |

# Features

- rename and re-order workspaces (task list?)
- tiling window manager
- scratch spaces
- nice keybind model + cheatsheet - capslock=mutate, cmd=navigate
- agent status - multi-harness, subagents, usage
- deck for central control (wip) - provides presence, cross-project context, etc
- everything cmdline driven via `bsctl`
- `make check`, `make drift` for auditing and adopting system changes

# `React` for your desktop (but QML) 

"hey claude i need to be able to do see xyz on my bar - make it happen"

# Want to try it? 

https://github.com/drshade/battlestation

"hey claude checkout this cool repo - how can i try it out?"
(there is an AGENTS.md and its designed to be re-used with tools like 'make check', 'make drift' etc)