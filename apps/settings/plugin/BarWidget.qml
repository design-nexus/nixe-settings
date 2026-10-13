import QtQuick
import qs.Ui

// Opens the Settings app (a separate GTK program); right-click jumps to Sound.
BarWidget {
  id: root
  moduleName: "design-nexus.settings-gear"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: ""
    horizontalMargin: 7.5
    onPressed: function(button) {
      if (!root.bar) return
      if (button === Qt.RightButton) root.bar.run("settings --section audio")
      else root.bar.run("settings")
    }
  }
}
