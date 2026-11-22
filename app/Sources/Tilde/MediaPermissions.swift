import AppKit
import WebKit

/// Which sites may use the camera and the microphone. A site is asked about
/// once; what was allowed is remembered, what was refused is asked again.
final class MediaPermissions {
    /// Site (scheme and host) to the devices it may use: "camera", "microphone".
    private var allowed: [String: Set<String>] = [:]
    private let file: URL

    init(file: URL) {
        self.file = file
        if let data = try? Data(contentsOf: file),
           let saved = try? JSONDecoder().decode([String: Set<String>].self, from: data) {
            allowed = saved
        }
    }

    private static func devices(_ type: WKMediaCaptureType) -> Set<String> {
        switch type {
        case .camera: return ["camera"]
        case .microphone: return ["microphone"]
        case .cameraAndMicrophone: return ["camera", "microphone"]
        @unknown default: return ["camera", "microphone"]
        }
    }

    private static func site(_ origin: WKSecurityOrigin) -> String {
        let standard = (origin.protocol == "https" && origin.port == 443) || (origin.protocol == "http" && origin.port == 80) || origin.port == 0
        return "\(origin.protocol)://\(origin.host)" + (standard ? "" : ":\(origin.port)")
    }

    /// Answers a page that wants the camera or the microphone, asking in a
    /// sheet on `window` unless the site was allowed them before.
    func decide(origin: WKSecurityOrigin, type: WKMediaCaptureType, window: NSWindow,
                handler: @escaping (WKPermissionDecision) -> Void) {
        // Only pages that arrived safely, or from this machine, are worth asking about.
        let local = ["localhost", "127.0.0.1", "::1"].contains(origin.host)
        guard origin.protocol == "https" || local, !origin.host.isEmpty else { return handler(.deny) }
        let site = Self.site(origin)
        let wanted = Self.devices(type)
        if wanted.isSubset(of: allowed[site] ?? []) { return handler(.grant) }

        let what = wanted.count == 2 ? "camera and microphone" : wanted.first!
        let alert = NSAlert()
        alert.messageText = "Allow \(origin.host) to use your \(what)?"
        alert.informativeText = "Tilde will remember this for \(origin.host). A tab that is using them is marked in the sidebar."
        alert.addButton(withTitle: "Allow")
        alert.addButton(withTitle: "Don’t Allow")
        alert.beginSheetModal(for: window) { [weak self] response in
            guard response == .alertFirstButtonReturn else {
                Log.write("media: refused \(what) to \(site)")
                return handler(.deny)
            }
            Log.write("media: allowed \(what) to \(site)")
            self?.allowed[site, default: []].formUnion(wanted)
            self?.save()
            handler(.grant)
        }
    }

    private func save() {
        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(allowed).write(to: file, options: .atomic)
        } catch {
            Log.write("cannot save media permissions: \(error.localizedDescription)")
        }
    }
}
