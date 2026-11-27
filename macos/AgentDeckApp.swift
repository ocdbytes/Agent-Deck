import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var statusItem: NSStatusItem!
    private var preview: NSRunningApplication?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)

        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = statusItem.button {
            button.image = NSImage(
                systemSymbolName: "rectangle.grid.2x2.fill",
                accessibilityDescription: "Agent Deck"
            )
        }

        let menu = NSMenu()
        menu.addItem(withTitle: "Open Preview", action: #selector(openPreview), keyEquivalent: "o")
        menu.addItem(.separator())
        menu.addItem(withTitle: "Quit Agent Deck", action: #selector(quit), keyEquivalent: "q")
        menu.items.forEach { $0.target = self }
        statusItem.menu = menu

        openPreview()
    }

    @objc private func openPreview() {
        if let preview, !preview.isTerminated {
            preview.activate(options: [.activateAllWindows])
            return
        }

        guard let url = Bundle.main.url(
            forResource: "Agent Deck Preview",
            withExtension: "app"
        ) else {
            showError("The preview application is missing.")
            return
        }

        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = true
        NSWorkspace.shared.openApplication(at: url, configuration: configuration) { [weak self] app, error in
            DispatchQueue.main.async {
                if let error {
                    self?.showError(error.localizedDescription)
                } else {
                    self?.preview = app
                }
            }
        }
    }

    private func showError(_ message: String) {
        let alert = NSAlert()
        alert.messageText = "Agent Deck could not open the preview"
        alert.informativeText = message
        alert.runModal()
    }

    @objc private func quit() {
        preview?.terminate()
        NSApp.terminate(nil)
    }

    func applicationWillTerminate(_ notification: Notification) {
        preview?.terminate()
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
