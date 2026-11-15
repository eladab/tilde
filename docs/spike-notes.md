# M0 spike notes

The spike was a throwaway AppKit app: a SwiftTerm view running a shell script
that painted a border of cells, and a `WKWebView` placed inside that border.
It has been deleted; everything it established is now in `app/`.

## Answers

**Does SwiftTerm expose exact cell size and grid origin?**
Not directly. `cellDimension` is internal and there is no public accessor for
the `Terminal`. What works:
- `TerminalView.cellSizeInPixels(source:)` is public and returns the cell in
  whole backing pixels. Dividing by the window's backing scale is exact,
  because SwiftTerm snaps cells to the pixel grid. Its `source` argument is
  unused, so a throwaway `Terminal` is passed.
- Grid size is `Int(bounds.width / cell.width)` by `Int(bounds.height /
  cell.height)`, the same arithmetic SwiftTerm uses.
- The grid starts at the view's top-left corner with no padding. The view is
  not flipped, so rows are converted from the top edge.
- With the default overlay scroller no width is reserved for it.

Measured: 8 × 16 pt cells at 2x with 13 pt system monospace. The web view frame
equals the cell rectangle exactly, checked numerically and by screenshot.
Only a 2x display was available; 1x and mixed-scale moves are untested.

**SwiftTerm rendering and features.**
Truecolor, SGR mouse reporting, bracketed paste and the alternate screen all
work with ratatui. Synchronized output and kitty graphics are present in the
source (1.99.3) but were not exercised. No rendering problems seen at the
redraw rates the TUI produces; a 60 Hz stress test was not run.

**libghostty.**
Not evaluated. SwiftTerm met every requirement on the first attempt, so the
time went into the app instead. `TerminalHost` is the seam if this is revisited;
the reason to revisit would be rendering quality or speed, not missing features.

**Does first responder move cleanly both ways?**
Yes, by `makeFirstResponder` and by mouse. One surprise: a web view takes first
responder by itself when the page focuses an element (`autofocus`, `focus()`),
which would pull the keyboard out of NAV mode whenever such a page loads. The
window now refuses a move into a web view unless it comes from a mouse click or
from the app's own mode switch (`BrowserWindow.makeFirstResponder`).

**Can the main menu win reserved shortcuts over page JavaScript?**
Yes. `BrowserWindow.performKeyEquivalent` offers the event to the main menu
before the responder chain. A page that calls `preventDefault()` on every key
event cannot block Cmd+T, Cmd+L or Cmd+W. Without the override a focused web
view claims every key equivalent first.
Ctrl+Shift+Tab does not match its menu item, so tab cycling is recognised by
key code in the same override.

**Does resizing keep the web view on its cell rectangle?**
Yes for programmatic resizes: the frame is recomputed from the last cell
rectangle on every resize, and again when the TUI sends the new one. The
window resizes in whole-cell increments. A live drag by hand has not been
watched for tearing.

**Does an unbundled executable host WKWebView?**
Yes: network, video and text input all work from `swift build` output. The app
is bundled anyway (`scripts/bundle.sh`) for the Info.plist and a Dock identity.

## Recommendation

Stay on SwiftTerm.
