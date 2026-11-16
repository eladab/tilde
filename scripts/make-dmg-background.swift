// Draws the background of the installer disk image's window: an arrow from
// where the app sits to where the Applications folder sits.
//
//     swift scripts/make-dmg-background.swift background.png
//
// The picture is 1280×800 pixels for a 640×400 point window; scripts/dmg.sh
// marks it as 144 dpi so the Finder shows it at that size. The icon positions
// here and in dmg.sh must agree.

import AppKit

let output = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "background.png"
let scale: CGFloat = 2
let (width, height): (CGFloat, CGFloat) = (640, 400)

func rgb(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: r / 255, green: g / 255, blue: b / 255, alpha: a)
}

let srgb = CGColorSpace(name: CGColorSpace.sRGB)!
let ctx = CGContext(data: nil, width: Int(width * scale), height: Int(height * scale), bitsPerComponent: 8, bytesPerRow: 0,
                    space: srgb, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
ctx.scaleBy(x: scale, y: scale)

// A light ground, so the Finder's black icon labels stay readable.
let ground = CGGradient(colorsSpace: srgb, colors: [rgb(246, 246, 250), rgb(228, 229, 238)] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(ground, start: CGPoint(x: 0, y: height), end: CGPoint(x: 0, y: 0), options: [])

// Icons are centred at x = 170 and x = 470, 190 points from the top.
let y = height - 190
let arrow = CGMutablePath()
arrow.move(to: CGPoint(x: 268, y: y))
arrow.addLine(to: CGPoint(x: 364, y: y))
ctx.setStrokeColor(rgb(120, 126, 150))
ctx.setLineWidth(7)
ctx.setLineCap(.round)
ctx.setLineJoin(.round)
ctx.addPath(arrow)
ctx.strokePath()
let head = CGMutablePath()
head.move(to: CGPoint(x: 346, y: y + 18))
head.addLine(to: CGPoint(x: 366, y: y))
head.addLine(to: CGPoint(x: 346, y: y - 18))
ctx.addPath(head)
ctx.strokePath()

NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: false)
let text = NSAttributedString(string: "Drag Tilde to Applications to install it", attributes: [
    .font: NSFont.systemFont(ofSize: 15, weight: .medium),
    .foregroundColor: NSColor(srgbRed: 90 / 255, green: 94 / 255, blue: 112 / 255, alpha: 1),
])
text.draw(at: CGPoint(x: (width - text.size().width) / 2, y: 52))

let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: output))
