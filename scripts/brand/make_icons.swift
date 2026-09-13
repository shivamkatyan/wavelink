// scripts/brand/make_icons.swift — generate the Wavelink mark at every size.
//
// Runs with `swift make_icons.swift <out-dir>` on macOS (AppKit + CoreGraphics,
// no external tooling). Draws the mark (white speaker + sound-wave arcs on the
// brand round-square) at 1024, then writes:
//   <out>/icon-1024.png                    master
//   <out>/AppIcon.iconset/*                macOS icon set -> iconutil .icns
//   <out>/AppIcon.appiconset/*             iOS asset-catalog icon set
//   <out>/wavelink-{512,256,128,64,48,32,16}.png   Linux/Windows raster
//   <out>/wavelink.ico                     256px Windows icon (PNG-compressed ICO)
// Also emits <out>/foreground.svg path hints in AndroidManifest docs (Android
// uses a plain adaptive-vector, authored by hand in res/ — see Phase B).
import AppKit
import Foundation

let out = CommandLine.arguments.count > 1
    ? URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
    : URL(fileURLWithPath: "brand-out", isDirectory: true)
try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

// Brand colours (UX_SPEC accent + surface).
let brand = NSColor(srgbRed: 0.039, green: 0.518, blue: 1.0, alpha: 1) // ~0A84FF
let glyph = NSColor.white

func drawMark(in ctx: NSGraphicsContext) {
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = ctx
    let s = ctx.cgContext.convertToDeviceSpace(CGSize(width: 1024, height: 1024))
    let canvas = CGRect(x: 0, y: 0, width: 1024, height: 1024)
    brand.setFill()
    ctx.cgContext.fill(canvas)

    let lineWidth: CGFloat = 46
    glyph.setStroke()
    glyph.setFill()

    // Speaker body + cone.
    let body = CGRect(x: 276, y: 438, width: 96, height: 256)
    NSBezierPath(roundedRect: body, xRadius: 18, yRadius: 18).fill()
    let cone = NSBezierPath()
    cone.move(to: NSPoint(x: 372, y: 438))
    cone.line(to: NSPoint(x: 372, y: 694))
    cone.line(to: NSPoint(x: 540, y: 566))
    cone.close()
    cone.fill()

    // Sound-wave arcs (open right-side half-arcs; round caps).
    func arc(_ cx: CGFloat, _ cy: CGFloat, _ r: CGFloat) -> NSBezierPath {
        let p = NSBezierPath()
        p.lineWidth = lineWidth
        p.lineCapStyle = .round
        // Right half-circle from (cx, cy-r) to (cx, cy+r).
        p.move(to: NSPoint(x: cx, y: cy - r))
        p.appendArc(withCenter: NSPoint(x: cx, y: cy), radius: r, startAngle: -90, endAngle: 90)
        return p
    }
    arc(610, 566, 118).stroke()
    arc(700, 566, 200).stroke()

    NSGraphicsContext.restoreGraphicsState()
}

func render(_ px: Int) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: px, height: px)
    let ctx = NSGraphicsContext(bitmapImageRep: rep)!
    drawMark(in: ctx)
    return rep
}

func writePNG(_ rep: NSBitmapImageRep, to name: String) {
    let url = out.appendingPathComponent(name)
    try? rep.representation(using: .png, properties: [:])?.write(to: url)
}

// Master + rasters.
writePNG(render(1024), to: "icon-1024.png")
for size in [512, 256, 128, 64, 48, 32, 16] {
    writePNG(render(size), to: "wavelink-\(size).png")
}

// macOS .icns (via iconutil).
let iconset = out.appendingPathComponent("AppIcon.iconset")
try? FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
let macSizes: [(Int, String)] = [
    (16, "icon_16x16"), (32, "icon_16x16@2x"), (32, "icon_32x32"),
    (64, "icon_32x32@2x"), (128, "icon_128x128"), (256, "icon_128x128@2x"),
    (256, "icon_256x256"), (512, "icon_256x256@2x"), (512, "icon_512x512"),
    (1024, "icon_512x512@2x"),
]
for (px, base) in macSizes {
    writePNG(render(px), to: "AppIcon.iconset/\(base).png")
}

// iOS AppIcon.appiconset (Contents.json + single-resolution PNGs).
let iosSet = out.appendingPathComponent("AppIcon.appiconset")
try? FileManager.default.createDirectory(at: iosSet, withIntermediateDirectories: true)
let iosSizes: [(Int, String)] = [
    (29, "Icon-App-29x29@1x"), (58, "Icon-App-29x29@2x"), (87, "Icon-App-29x29@3x"),
    (20, "Icon-App-20x20@1x"), (40, "Icon-App-20x20@2x"), (60, "Icon-App-20x20@3x"),
    (40, "Icon-App-40x40@1x"), (80, "Icon-App-40x40@2x"), (120, "Icon-App-40x40@3x"),
    (60, "Icon-App-60x60@2x"), (180, "Icon-App-60x60@3x"),
    (76, "Icon-App-76x76@1x"), (152, "Icon-App-76x76@2x"),
    (167, "Icon-App-83.5x83.5@2x"),
]
for (px, base) in iosSizes {
    if FileManager.default.fileExists(atPath: iosSet.appendingPathComponent("\(base).png").path) { continue }
    writePNG(render(px), to: "AppIcon.appiconset/\(base).png")
}
let contents: [String: Any] = [
    "images": iosSizes.map { (px, base) -> [String: String] in
        var idiom = "iphone", scale = "1x"
        if px > 76 { idiom = "ipad"; scale = "2x" } else if px == 167 || px == 152 { idiom = "ipad"; scale = "2x" }
        if px >= 120 { scale = "3x" } else if px > 60 { scale = "2x" } else if px == 60 { scale = "2x" } else if px == 80 || px == 58 || px == 40 || px == 20 || px == 29 { scale = "2x" }
        return ["idiom": idiom, "filename": "\(base).png", "scale": scale, "size": "\(Float(px) / 3.0)x\(Float(px) / 3.0)"]
    },
    "info": ["version": 1, "author": "xcode"],
]
if let data = try? JSONSerialization.data(withJSONObject: contents, options: [.prettyPrinted]),
    !FileManager.default.fileExists(atPath: iosSet.appendingPathComponent("Contents.json").path) {
    try? data.write(to: iosSet.appendingPathComponent("Contents.json"))
}

// Windows .ico (256px PNG stored inside an ICO container — valid for Vista+).
do {
    let png = render(256).representation(using: .png, properties: [:])!
    var header = Data()
    header.append(contentsOf: [0x00, 0x00, 0x01, 0x00, 0x01, 0x00]) // ICONDIR: reserved, type=1, count=1
    header.append(contentsOf: [0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x20, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00]) // entry
    var size = UInt32(png.count).littleEndian
    withUnsafeBytes(of: &size) { header.append(contentsOf: Array($0)) }
    var offset = UInt32(6 + 16).littleEndian
    withUnsafeBytes(of: &offset) { header.append(contentsOf: Array($0)) }
    var ico = Data()
    ico.append(header)
    ico.append(png)
    try ico.write(to: out.appendingPathComponent("wavelink.ico"))
}

print("wavelink icons written to \(out.path)")
