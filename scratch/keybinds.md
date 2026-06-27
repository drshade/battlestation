# Hyprland keybinds (review draft)

Source of truth: stow/home/hypr/.config/hypr/config/keybinds.lua
This is a review artifact — edit freely, then we fold changes back into the .lua.
Mark edits however you like (strike, ADD:, CHANGE:, ?).

## Window management
| Keys                    | Action                                       |
| ----------------------- | -------------------------------------------- |
| SUPER Escape            | Kill mode — click a window to force-kill     |
| SUPER Q                 | Close active window                          |
| SUPER ALT Space         | Toggle floating                              |
| SUPER D                 | Maximize (fullscreen mode 1, keeps bar/gaps) |
| SUPER F                 | Fullscreen (true)                            |
| SUPER J                 | Toggle split direction                       |
| SUPER L                 | Lock screen                                  |
| SUPER ALT C             | Session menu                                 |
| SUPER ←/→/↑/↓           | Focus left/right/up/down                     |
| ALT Tab                 | Cycle to next window                         |
| SUPER SHIFT ←/→/↑/↓     | Move window within workspace                 |
| SUPER CONTROL SHIFT ←/→ | Move window to adjacent workspace            |
| SUPER LMB (drag)        | Move window                                  |
| SUPER RMB (drag)        | Resize window                                |

## Launch
| Keys                 | Action                          |
| -------------------- | ------------------------------- |
| SUPER Return         | Terminal (kitty)                |
| SUPER E              | File manager (dolphin)          |
| SUPER T              | Text editor (gnome-text-editor) |
| SUPER C              | Calculator (gnome-calculator)   |
| SUPER W              | Browser (firefox)               |
| CONTROL SHIFT Escape | btop (in kitty)                 |
| SUPER Space          | App launcher                    |
| SUPER period         | Emoji picker                    |
| SUPER V              | Clipboard history               |
| SUPER Z              | Noctalia settings               |
| SUPER X              | Noctalia control center         |

## Workspaces
| Keys                  | Action                                     |
| --------------------- | ------------------------------------------ |
| SUPER 1-0             | Switch to workspace 1-10                   |
| SUPER SHIFT 1-0       | Move window to workspace 1-10 (and follow) |
| SUPER ALT 1-0         | Send window to workspace 1-10 (stay)       |
| SUPER CONTROL →/←     | Next / previous workspace                  |
| SUPER CONTROL ↓       | First empty workspace                      |
| SUPER CONTROL ALT →/← | Move window to next / previous workspace   |
| SUPER scroll          | Next / previous workspace                  |
| SUPER S               | Toggle scratchpad (special workspace)      |
| SUPER SHIFT S         | Send window to scratchpad                  |

## Utilities / capture
| Keys          | Action                          |
| ------------- | ------------------------------- |
| SUPER P       | Color picker                    |
| Print         | Annotate screenshot             |
| SUPER Print   | Annotate active window          |
| SUPER R       | Screen toolkit (recording etc.) |
| SUPER SHIFT W | Cycle wallpaper                 |
| SUPER A       | Notification history            |

## Hardware keys
| Keys                           | Action                  |
| ------------------------------ | ----------------------- |
| XF86Audio Raise/Lower/Mute     | Volume up / down / mute |
| XF86AudioMicMute               | Mute microphone         |
| XF86Audio Play/Pause/Next/Prev | Media controls          |
| XF86MonBrightness Up/Down      | Brightness up / down    |

## Notes / questions
- REDUNDANT: `SUPER CONTROL SHIFT ←/→` and `SUPER CONTROL ALT ←/→` both = move window to adjacent workspace.
