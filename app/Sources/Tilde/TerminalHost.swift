import AppKit
import SwiftTerm

/// A rectangle of terminal cells, origin at the top-left cell.
struct CellRect: Equatable {
    var col: Int
    var row: Int
    var cols: Int
    var rows: Int
}

/// The embedded terminal, hidden behind the few things the browser needs from it.
protocol TerminalHost: AnyObject {
    var view: NSView { get }
    /// Size of one cell in points.
    var cellSize: CGSize { get }
    /// Current grid size in cells.
    var gridSize: (cols: Int, rows: Int) { get }
    var isProcessRunning: Bool { get }
    /// Process ID of the running child, if any.
    var processID: Int32? { get }

    /// Called on the main queue when the child exits. `nil` means killed by a signal.
    var onExit: ((Int32?) -> Void)? { get set }
    /// Called when the grid changes size.
    var onResize: (() -> Void)? { get set }
    /// Called when the user types while no child process is running.
    var onInputWhileStopped: (() -> Void)? { get set }

    /// Frame of a cell rectangle in the coordinate space of `view.superview`.
    func frame(for cells: CellRect) -> NSRect
    func start(executable: String, args: [String], environment: [String: String])
    func terminateProcess()
    /// Show text in the terminal without sending it to the child.
    func display(_ text: String)
    /// Deliver bytes to the child as if typed.
    func type(_ text: String)
}

private final class TildeTerminalView: LocalProcessTerminalView {
    var inputWhileStopped: (() -> Void)?

    override func send(source: TerminalView, data: ArraySlice<UInt8>) {
        if process?.running == true {
            super.send(source: source, data: data)
        } else {
            inputWhileStopped?()
        }
    }
}

final class SwiftTermHost: NSObject, TerminalHost, LocalProcessTerminalViewDelegate {
    private let term: TildeTerminalView
    private lazy var probe = Terminal(delegate: term)

    var onExit: ((Int32?) -> Void)?
    var onResize: (() -> Void)?
    var onInputWhileStopped: (() -> Void)? {
        didSet { term.inputWhileStopped = onInputWhileStopped }
    }

    var view: NSView { term }

    init(font: NSFont) {
        term = TildeTerminalView(frame: NSRect(x: 0, y: 0, width: 800, height: 480))
        super.init()
        term.font = font
        term.nativeBackgroundColor = NSColor(srgbRed: 0.09, green: 0.09, blue: 0.11, alpha: 1)
        term.nativeForegroundColor = NSColor(srgbRed: 0.86, green: 0.87, blue: 0.89, alpha: 1)
        term.processDelegate = self
    }

    var cellSize: CGSize {
        // SwiftTerm keeps its cell size internal but publishes it in whole
        // pixels, which is exact because it snaps cells to the pixel grid.
        let scale = term.window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2
        if let px = term.cellSizeInPixels(source: probe), px.width > 0, px.height > 0 {
            return CGSize(width: CGFloat(px.width) / scale, height: CGFloat(px.height) / scale)
        }
        let optimal = term.getOptimalFrameSize()
        return CGSize(width: max(1, optimal.width / 80), height: max(1, optimal.height / 25))
    }

    var gridSize: (cols: Int, rows: Int) {
        let cell = cellSize
        return (Int(term.bounds.width / cell.width), Int(term.bounds.height / cell.height))
    }

    var isProcessRunning: Bool { term.process?.running == true }

    var processID: Int32? { isProcessRunning ? term.process?.shellPid : nil }

    func frame(for cells: CellRect) -> NSRect {
        let cell = cellSize
        let bounds = term.bounds
        let x = CGFloat(cells.col) * cell.width
        let top = CGFloat(cells.row) * cell.height
        let width = min(CGFloat(cells.cols) * cell.width, max(0, bounds.width - x))
        let height = min(CGFloat(cells.rows) * cell.height, max(0, bounds.height - top))
        // The grid starts at the view's top-left corner; AppKit's origin is bottom-left.
        let y = term.isFlipped ? top : bounds.height - top - height
        return NSRect(x: term.frame.minX + x, y: term.frame.minY + y, width: width, height: height)
    }

    func start(executable: String, args: [String], environment: [String: String]) {
        let env = environment.map { "\($0.key)=\($0.value)" }
        term.startProcess(executable: executable, args: args, environment: env)
    }

    func terminateProcess() {
        if isProcessRunning { term.terminate() }
    }

    func display(_ text: String) {
        term.feed(text: text)
    }

    func type(_ text: String) {
        let bytes = Array(text.utf8)
        term.send(source: term, data: bytes[...])
    }

    // MARK: LocalProcessTerminalViewDelegate

    func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {
        onResize?()
    }

    func setTerminalTitle(source: LocalProcessTerminalView, title: String) {}

    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}

    func processTerminated(source: TerminalView, exitCode: Int32?) {
        onExit?(exitCode)
    }
}
