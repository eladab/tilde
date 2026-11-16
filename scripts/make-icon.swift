// Draws the Tilde app icon and writes it as a 1024×1024 PNG.
//
//     swift scripts/make-icon.swift icon.png
//
// scripts/make-icon.sh turns that into app/Tilde.icns.
//
// The picture is a tilde, the character a shell uses for "home", drawn as a
// single heavy stroke of glowing phosphor green on a dark terminal screen.

import AppKit

let size: CGFloat = 1024
let output = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icon.png"

func rgb(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: r / 255, green: g / 255, blue: b / 255, alpha: a)
}

let srgb = CGColorSpace(name: CGColorSpace.sRGB)!
let ctx = CGContext(data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0,
                    space: srgb, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!

func gradient(_ colors: [CGColor], _ locations: [CGFloat]? = nil) -> CGGradient {
    CGGradient(colorsSpace: srgb, colors: colors as CFArray, locations: locations)!
}

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

// The tilde: one stroke, rising into a crest and falling into a trough,
// centred on the plate. (Origin is bottom-left.)
let centre = CGPoint(x: 512, y: 512)
let span: CGFloat = 520
let weight: CGFloat = 136
let tilde: CGPath = {
    // The curve is drawn on a 396-wide grid and scaled; it stands a little
    // taller than it is drawn, so it fills the plate.
    let s = span / 396
    func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: centre.x + (x - 458) * s, y: centre.y + (y - 518) * s * 1.3) }
    let path = CGMutablePath()
    path.move(to: p(260, 478))
    path.addCurve(to: p(458, 518), control1: p(309, 604), control2: p(396, 592))
    path.addCurve(to: p(656, 558), control1: p(520, 444), control2: p(607, 432))
    return path
}()

func strokeTilde(width: CGFloat) {
    ctx.setLineWidth(width)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)
    ctx.addPath(tilde)
}

ctx.saveGState()
ctx.addPath(platePath)
ctx.clip()

// The screen: near black with a green cast, lit from behind the glyph.
ctx.drawLinearGradient(gradient([rgb(10, 30, 26), rgb(2, 8, 8)]), start: CGPoint(x: 512, y: 924), end: CGPoint(x: 512, y: 100), options: [])
ctx.drawRadialGradient(gradient([rgb(60, 255, 170, 0.30), rgb(60, 255, 170, 0)]), startCenter: centre, startRadius: 0,
                       endCenter: centre, endRadius: 500, options: [])

// The glow the stroke throws on the screen.
ctx.saveGState()
ctx.setShadow(offset: .zero, blur: 90, color: rgb(70, 255, 170, 0.85))
strokeTilde(width: weight - 4)
ctx.setStrokeColor(rgb(70, 255, 170, 0.85))
ctx.strokePath()
ctx.restoreGState()

// The stroke itself, from yellow-green at its crest to teal at its tail.
ctx.saveGState()
strokeTilde(width: weight)
ctx.replacePathWithStrokedPath()
ctx.clip()
ctx.drawLinearGradient(gradient([rgb(210, 255, 120), rgb(70, 245, 160), rgb(30, 215, 205)], [0, 0.5, 1]),
                       start: CGPoint(x: centre.x - span / 2, y: 660), end: CGPoint(x: centre.x + span / 2, y: 380),
                       options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
ctx.restoreGState()

// Scan lines, over everything on the screen.
ctx.setFillColor(rgb(0, 0, 0, 0.13))
var line: CGFloat = 100
while line < 924 {
    ctx.fill(CGRect(x: 100, y: line, width: 824, height: 5))
    line += 14
}

// A hairline highlight around the plate's edge.
ctx.addPath(CGPath(roundedRect: plate.insetBy(dx: 2, dy: 2), cornerWidth: 184, cornerHeight: 184, transform: nil))
ctx.setLineWidth(4)
ctx.setStrokeColor(rgb(255, 255, 255, 0.12))
ctx.strokePath()
ctx.restoreGState()

let image = ctx.makeImage()!
let rep = NSBitmapImageRep(cgImage: image)
let data = rep.representation(using: .png, properties: [:])!
try! data.write(to: URL(fileURLWithPath: output))
