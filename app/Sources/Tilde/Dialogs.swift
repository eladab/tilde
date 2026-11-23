import AppKit
import SecurityInterface
import WebKit

/// What a page asks of the person in front of it: the dialogs of its script
/// (`alert`, `confirm`, `prompt`), a file to upload, a name and password, a
/// certificate. Each is a sheet on the window, and a page in a background
/// tab says which site it is.
extension TabStore {
    private func site(of webView: WKWebView, frame: WKFrameInfo? = nil) -> String {
        let host = frame?.securityOrigin.host ?? ""
        if !host.isEmpty { return host }
        return webView.url?.host ?? webView.url?.lastPathComponent ?? "This page"
    }

    /// A page that puts up one dialog after another can be told to stop:
    /// from the third on, the sheet offers to show no more until the tab
    /// goes to another page. Answers false when the tab's dialogs are off.
    private func mayAsk(_ webView: WKWebView, _ alert: NSAlert) -> Bool {
        guard let tab = tab(for: webView), presenter?() != nil else { return false }
        if tab.dialogsBlocked { return false }
        tab.dialogCount += 1
        if tab.dialogCount >= 3 {
            alert.showsSuppressionButton = true
            alert.suppressionButton?.title = "Show no more dialogs from this page"
        }
        return true
    }

    private func answered(_ webView: WKWebView, _ alert: NSAlert) {
        if alert.suppressionButton?.state == .on { tab(for: webView)?.dialogsBlocked = true }
    }

    func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping () -> Void) {
        let alert = NSAlert()
        alert.messageText = "\(site(of: webView, frame: frame)) says"
        alert.informativeText = String(message.prefix(2000))
        alert.addButton(withTitle: "OK")
        guard mayAsk(webView, alert), let window = presenter?() else {
            Log.write("js alert not shown: \(message.prefix(200))")
            return completionHandler()
        }
        alert.beginSheetModal(for: window) { [weak self] _ in
            self?.answered(webView, alert)
            completionHandler()
        }
    }

    func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (Bool) -> Void) {
        let alert = NSAlert()
        alert.messageText = "\(site(of: webView, frame: frame)) asks"
        alert.informativeText = String(message.prefix(2000))
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        guard mayAsk(webView, alert), let window = presenter?() else {
            Log.write("js confirm not shown: \(message.prefix(200))")
            return completionHandler(false)
        }
        alert.beginSheetModal(for: window) { [weak self] response in
            self?.answered(webView, alert)
            completionHandler(response == .alertFirstButtonReturn)
        }
    }

    func webView(_ webView: WKWebView, runJavaScriptTextInputPanelWithPrompt prompt: String, defaultText: String?,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (String?) -> Void) {
        let alert = NSAlert()
        alert.messageText = "\(site(of: webView, frame: frame)) asks"
        alert.informativeText = String(prompt.prefix(2000))
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 320, height: 24))
        field.stringValue = defaultText ?? ""
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard mayAsk(webView, alert), let window = presenter?() else {
            Log.write("js prompt not shown: \(prompt.prefix(200))")
            return completionHandler(nil)
        }
        alert.beginSheetModal(for: window) { [weak self] response in
            self?.answered(webView, alert)
            completionHandler(response == .alertFirstButtonReturn ? field.stringValue : nil)
        }
    }

    /// `<input type=file>`: the system's file picker.
    func webView(_ webView: WKWebView, runOpenPanelWith parameters: WKOpenPanelParameters,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping ([URL]?) -> Void) {
        guard let window = presenter?() else { return completionHandler(nil) }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = parameters.allowsMultipleSelection
        panel.canChooseDirectories = parameters.allowsDirectories
        panel.canChooseFiles = true
        panel.message = "Choose what to give \(site(of: webView, frame: frame))"
        panel.beginSheetModal(for: window) { response in
            completionHandler(response == .OK ? panel.urls : nil)
        }
    }

    // MARK: Names, passwords and certificates

    func webView(_ webView: WKWebView, didReceive challenge: URLAuthenticationChallenge,
                 completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        let space = challenge.protectionSpace
        switch space.authenticationMethod {
        case NSURLAuthenticationMethodHTTPBasic, NSURLAuthenticationMethodHTTPDigest, NSURLAuthenticationMethodNTLM,
             NSURLAuthenticationMethodNegotiate:
            askForPassword(challenge, completionHandler: completionHandler)
        case NSURLAuthenticationMethodClientCertificate:
            askForCertificate(challenge, completionHandler: completionHandler)
        default:
            completionHandler(.performDefaultHandling, nil)
        }
    }

    private func askForPassword(_ challenge: URLAuthenticationChallenge,
                                completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        let space = challenge.protectionSpace
        // What was typed earlier in this run is tried once before asking again.
        if challenge.previousFailureCount == 0, let known = challenge.proposedCredential, known.hasPassword {
            return completionHandler(.useCredential, known)
        }
        guard let window = presenter?(), challenge.previousFailureCount < 5 else {
            return completionHandler(.cancelAuthenticationChallenge, nil)
        }
        let alert = NSAlert()
        alert.messageText = "\(space.host) asks for a name and password"
        var notes: [String] = []
        if let realm = space.realm, !realm.isEmpty { notes.append("It says: “\(realm.prefix(200))”.") }
        if challenge.previousFailureCount > 0 { notes.append("That name and password were not accepted.") }
        if !space.receivesCredentialSecurely { notes.append("They will be sent without encryption.") }
        alert.informativeText = notes.joined(separator: " ")
        alert.addButton(withTitle: "Sign In")
        alert.addButton(withTitle: "Cancel")
        let name = NSTextField(frame: NSRect(x: 0, y: 30, width: 280, height: 24))
        name.placeholderString = "Name"
        name.stringValue = challenge.proposedCredential?.user ?? ""
        let password = NSSecureTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24))
        password.placeholderString = "Password"
        let fields = NSView(frame: NSRect(x: 0, y: 0, width: 280, height: 54))
        fields.addSubview(name)
        fields.addSubview(password)
        name.nextKeyView = password
        alert.accessoryView = fields
        alert.window.initialFirstResponder = name
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return completionHandler(.cancelAuthenticationChallenge, nil) }
            // Kept for as long as Tilde runs, not on disk.
            completionHandler(.useCredential, URLCredential(user: name.stringValue, password: password.stringValue, persistence: .forSession))
        }
    }

    /// A site that wants a certificate is shown the identities in the
    /// keychain to choose from. With none, the site is told there is none.
    private func askForCertificate(_ challenge: URLAuthenticationChallenge,
                                   completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        let query: [CFString: Any] = [kSecClass: kSecClassIdentity, kSecMatchLimit: kSecMatchLimitAll, kSecReturnRef: true]
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let identities = result as? [SecIdentity], !identities.isEmpty, let window = presenter?() else {
            return completionHandler(.performDefaultHandling, nil)
        }
        let panel = SFChooseIdentityPanel.shared()!
        panel.setAlternateButtonTitle("Cancel")
        panel.setInformativeText("\(challenge.protectionSpace.host) asks for a certificate that says who you are.")
        certificateAnswer = { [weak panel] chosen in
            guard chosen, let identity = panel?.identity()?.takeUnretainedValue() else {
                return completionHandler(.cancelAuthenticationChallenge, nil)
            }
            completionHandler(.useCredential, URLCredential(identity: identity, certificates: nil, persistence: .forSession))
        }
        panel.beginSheet(for: window, modalDelegate: self,
                         didEnd: #selector(certificatePanelDidEnd(_:returnCode:contextInfo:)), contextInfo: nil,
                         identities: identities, message: "Choose a certificate")
    }

    @objc func certificatePanelDidEnd(_ sheet: NSWindow, returnCode: Int, contextInfo: UnsafeMutableRawPointer?) {
        let answer = certificateAnswer
        certificateAnswer = nil
        answer?(returnCode == NSApplication.ModalResponse.OK.rawValue)
    }
}
