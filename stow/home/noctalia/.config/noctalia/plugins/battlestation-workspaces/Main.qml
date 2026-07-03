// Single plugin instance: owns the rename + settings flows and the IPC entry
// point the Hyprland keybind calls. Both reuse Noctalia's plugin-panel system so
// the panel hangs off the bar (attached blob + open animation) like the native
// panels; Panel.qml is the panel content. The mode and rename target ride on
// panelMode/pendingRenameId/Name, which the panel reads when it loads.
import QtQuick
import Quickshell
import Quickshell.Io
import qs.Services.Compositor

Item {
  id: root
  property var pluginApi: null

  // What the bar panel should show next ("rename" | "settings"), and the rename
  // target, both read by Panel.qml when it opens.
  property string panelMode: "rename"
  property int pendingRenameId: 0
  property string pendingRenameName: ""

  // Per-screen bar widget items, so the panel can attach next to the widget on
  // the right screen (keybind path has no button of its own to anchor to).
  property var barItems: ({})
  function registerBar(screenName, item) {
    var m = barItems;
    m[screenName] = item;
    barItems = m;
  }
  function unregisterBar(screenName) {
    var m = barItems;
    delete m[screenName];
    barItems = m;
  }

  // Stage the target, then open the bar panel near buttonItem.
  function openRenamePanel(screen, buttonItem, wsId, wsName) {
    panelMode = "rename";
    pendingRenameId = wsId;
    pendingRenameName = wsName || "";
    if (pluginApi)
      pluginApi.openPanel(screen, buttonItem);
  }

  // Open the widget settings in the same bar panel.
  function openSettingsPanel(screen, buttonItem) {
    panelMode = "settings";
    if (pluginApi)
      pluginApi.openPanel(screen, buttonItem);
  }

  // Rename whichever workspace is currently focused (used by the keybind / IPC).
  function renameActive() {
    if (!pluginApi)
      return;
    pluginApi.withCurrentScreen(function (screen) {
      for (var i = 0; i < CompositorService.workspaces.count; i++) {
        var w = CompositorService.workspaces.get(i);
        if (w.isFocused === true) {
          root.openRenamePanel(screen, root.barItems[screen.name] || null, w.id, w.name);
          return;
        }
      }
    });
  }

  // `qs -c noctalia-shell ipc call plugin:battlestation-workspaces rename` (Super+Shift+R).
  IpcHandler {
    target: "plugin:battlestation-workspaces"
    function rename() {
      root.renameActive();
    }
  }
}
