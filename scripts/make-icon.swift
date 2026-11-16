// Draws the Tilde app icon and writes it as a 1024×1024 PNG.
//
//     swift scripts/make-icon.swift icon.png
//
// scripts/make-icon.sh turns that into app/Tilde.icns.
//
// The picture is a tilde, the character a shell uses for "home", drawn as a
// single bold stroke on a dark plate, followed by a block cursor.

import AppKit

let size: CGFloat = 1024
let output = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icon.png"

func rgb(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: r / 255, green: g / 255, blue: b / 255, alpha: a)
}

let srgb = CGColorSpace(name: CGColorSpace.sRGB)!
let ctx = CGContext(data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0,
                    space: srgb, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!

// macOS icon shape: an 824 pt rounded square centred on the 1024 pt canvas.
let plate = CGRect(x: 100, y: 100, width: 824, height: 824)
let platePath = CGPath(roundedRect: plate, cornerWidth: 186, cornerHeight: 186, transform: nil)

// The plate, with the soft drop shadow macOS icons carry.
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 24, color: rgb(0, 0, 0, 0.35))
ctx.addPath(platePath)
ctx.setFillColor(rgb(20, 21, 28))
ctx.fillPath()
ctx.restoreGState()

ctx.saveGState()
ctx.addPath(platePath)
ctx.clip()
let plateGradient = CGGradient(colorsSpace: srgb, colors: [rgb(40, 43, 58), rgb(17, 18, 24)] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(plateGradient, start: CGPoint(x: 512, y: 924), end: CGPoint(x: 512, y: 100), options: [])
// A faint glow behind the glyph gives the plate some depth.
let glow = CGGradient(colorsSpace: srgb, colors: [rgb(110, 140, 255, 0.16), rgb(110, 140, 255, 0)] as CFArray, locations: [0, 1])!
ctx.drawRadialGradient(glow, startCenter: CGPoint(x: 512, y: 520), startRadius: 0,
                       endCenter: CGPoint(x: 512, y: 520), endRadius: 420, options: [])
ctx.restoreGState()

// The tilde: one stroke, rising into a crest and falling into a trough.
// (Origin is bottom-left.)
let tilde = CGMutablePath()
tilde.move(to: CGPoint(x: 260, y: 478))
tilde.addCurve(to: CGPoint(x: 458, y: 518), control1: CGPoint(x: 309, y: 604), control2: CGPoint(x: 396, y: 592))
tilde.addCurve(to: CGPoint(x: 656, y: 558), control1: CGPoint(x: 520, y: 444), control2: CGPoint(x: 607, y: 432))
let strokeWidth: CGFloat = 86

func strokeTilde() {
    ctx.setLineWidth(strokeWidth)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)
    ctx.addPath(tilde)
}

// Its shadow on the plate.
ctx.saveGState()
ctx.addPath(platePath)
ctx.clip()
ctx.setShadow(offset: CGSize(width: 0, height: -14), blur: 30, color: rgb(0, 0, 0, 0.45))
strokeTilde()
ctx.setStrokeColor(rgb(120, 150, 245))
ctx.strokePath()
ctx.restoreGState()

// The stroke itself, filled with a gradient from sky blue to violet.
ctx.saveGState()
strokeTilde()
ctx.replacePathWithStrokedPath()
ctx.clip()
let ink = CGGradient(colorsSpace: srgb, colors: [rgb(104, 196, 255), rgb(128, 150, 250), rgb(176, 132, 246)] as CFArray,
                     locations: [0, 0.5, 1])!
ctx.drawLinearGradient(ink, start: CGPoint(x: 260, y: 600), end: CGPoint(x: 656, y: 440),
                       options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
// A light edge along the top of the stroke.
let sheen = CGGradient(colorsSpace: srgb, colors: [rgb(255, 255, 255, 0.22), rgb(255, 255, 255, 0)] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(sheen, start: CGPoint(x: 458, y: 640), end: CGPoint(x: 458, y: 500), options: [])
ctx.restoreGState()

// The cursor: a block standing after the tilde, as at a prompt.
// Set apart from the stroke by about half its own width, so the two read as
// separate characters rather than one shape.
let cursor = CGRect(x: 742, y: 418, width: 66, height: 200)
let cursorPath = CGPath(roundedRect: cursor, cornerWidth: 14, cornerHeight: 14, transform: nil)
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -14), blur: 30, color: rgb(0, 0, 0, 0.45))
ctx.addPath(cursorPath)
ctx.setFillColor(rgb(236, 238, 246))
ctx.fillPath()
ctx.restoreGState()
ctx.saveGState()
ctx.addPath(cursorPath)
ctx.clip()
let cursorGradient = CGGradient(colorsSpace: srgb, colors: [rgb(250, 251, 255), rgb(208, 212, 228)] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(cursorGradient, start: CGPoint(x: 775, y: 618), end: CGPoint(x: 775, y: 418), options: [])
ctx.restoreGState()

// A hairline highlight around the plate's edge.
ctx.saveGState()
ctx.addPath(platePath)
ctx.clip()
ctx.addPath(CGPath(roundedRect: plate.insetBy(dx: 2, dy: 2), cornerWidth: 184, cornerHeight: 184, transform: nil))
ctx.setLineWidth(4)
ctx.setStrokeColor(rgb(255, 255, 255, 0.08))
ctx.strokePath()
ctx.restoreGState()

let image = ctx.makeImage()!
let rep = NSBitmapImageRep(cgImage: image)
let data = rep.representation(using: .png, properties: [:])!
try! data.write(to: URL(fileURLWithPath: output))
