import AppKit

// render_symbol.swift <symbolName> <pointSize> <outPath>
// Рендерит РЕАЛЬНЫЙ SF Symbol как чёрный силуэт на прозрачном фоне (для template).
// @2x: рисуем в pixel-буфер удвоенного размера.

let args = CommandLine.arguments
guard args.count == 4, let pt = Double(args[2]) else {
    FileHandle.standardError.write("usage: render_symbol <name> <pt> <out>\n".data(using: .utf8)!)
    exit(1)
}
let name = args[1]
let outPath = args[3]
let scale: CGFloat = 2.0  // retina

let config = NSImage.SymbolConfiguration(pointSize: CGFloat(pt), weight: .regular)
guard let base = NSImage(systemSymbolName: name, accessibilityDescription: nil),
      let symbol = base.withSymbolConfiguration(config) else {
    FileHandle.standardError.write("symbol not found: \(name)\n".data(using: .utf8)!)
    exit(2)
}

let size = symbol.size
let pxW = Int(size.width * scale)
let pxH = Int(size.height * scale)

guard let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: pxW, pixelsHigh: pxH,
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else { exit(3) }
rep.size = size

NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
// чёрным по прозрачному — силуэт для template
NSColor.black.set()
let rect = NSRect(origin: .zero, size: size)
symbol.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1.0)
NSGraphicsContext.restoreGraphicsState()

guard let png = rep.representation(using: .png, properties: [:]) else { exit(4) }
try! png.write(to: URL(fileURLWithPath: outPath))
print("\(name): \(pxW)x\(pxH)px (\(Int(size.width))x\(Int(size.height))pt @2x)")
