// Single plugin instance: owns the rename + settings flows and the IPC entry
// point the Hyprland keybind calls. Both reuse Noctalia's plugin-panel system so
// the panel hangs off the bar (attached blob + open animation) like the native
// panels; Panel.qml is the panel content. The mode and rename target ride on
// panelMode/pendingRenameId/Name, which the panel reads when it loads.
import QtQuick
import Quickshell
import Quickshell.Io
import qs.Services.Compositor
import qs.Services.UI

Item {
  id: root
  property var pluginApi: null

  // What the bar panel should show next ("rename" | "settings" | "asks"), and
  // the rename target, both read by Panel.qml when it opens.
  property string panelMode: "rename"
  property int pendingRenameId: 0
  property string pendingRenameName: ""

  // Live asks rows, pushed by whichever bar instance's stream last changed —
  // every bar carries identical world state, so last-writer-wins is sound.
  // The asks panel (recreated on every open) binds to this for live updates.
  property var asksRows: []
  // session id -> live status (waiting/thinking/tooling), pushed alongside
  // asksRows by the bar's stream. The asks panel joins an ask's `session`
  // against this to label its answer button Trigger (idle asker → a kitty
  // wake fires) vs Enqueue (busy asker → the harness collects it later).
  property var statusBySid: ({})

  // Toast NEW notify-type asks (an FYI's whole point is being seen — the
  // badge alone is too quiet for it). This singleton is the dedupe point:
  // every bar instance pushes identical rows here, so the toasted-id set
  // makes the second bar's push (and every later echo) a no-op. The FIRST
  // push after instantiation seeds the set WITHOUT toasting — a shell
  // restart must not toast the whole standing queue — and only asks that
  // arrive OPEN toast (an already-answered notify surfacing in a later
  // diff is history, not news). Questions/reviews stay deliberately
  // quiet: push signals for them are escalation policy, later.
  property var toastedNotifyIds: ({})
  property bool notifyToastSeeded: false
  onAsksRowsChanged: {
    var seen = toastedNotifyIds;
    for (var i = 0; i < asksRows.length; i++) {
      var r = asksRows[i];
      if (r.type !== "notify" || seen[String(r.id)])
        continue;
      seen[String(r.id)] = true;
      if (notifyToastSeeded && r.state === "open")
        ToastService.showNotice(r.title || "New notify", r.body || "", "info-circle");
    }
    toastedNotifyIds = seen;
    notifyToastSeeded = true;
  }

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

  // Toggle the asks panel (badge click + the SUPER+A keybind). Exact toggle
  // semantics built from openPanel/closePanel rather than togglePanel: the
  // mode must be staged BEFORE an open, and a toggle of a panel currently
  // showing another mode should just close it.
  function toggleAsksPanel(screen, buttonItem) {
    if (!pluginApi)
      return;
    if (pluginApi.panelOpenScreen) {
      pluginApi.closePanel(pluginApi.panelOpenScreen);
      return;
    }
    panelMode = "asks";
    pluginApi.openPanel(screen, buttonItem || barItems[screen.name] || null);
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

  // `qs -c noctalia-shell ipc call plugin:battlestation-workspaces <fn>`:
  // rename (HYPER+R) and the asks-panel toggle (HYPER+A).
  IpcHandler {
    target: "plugin:battlestation-workspaces"
    function rename() {
      root.renameActive();
    }
    function asks() {
      if (!root.pluginApi)
        return;
      root.pluginApi.withCurrentScreen(function (screen) {
        root.toggleAsksPanel(screen, root.barItems[screen.name] || null);
      });
    }
  }
}
