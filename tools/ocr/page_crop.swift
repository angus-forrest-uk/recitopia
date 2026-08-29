import CoreGraphics
import CoreImage
import Foundation
import ImageIO
import UniformTypeIdentifiers
import Vision

struct PointReport: Encodable {
    let x: Double
    let y: Double
}

struct CropReport: Encodable {
    let inputPath: String
    let outputPath: String
    let didCrop: Bool
    let method: String
    let sourceWidth: Int
    let sourceHeight: Int
    let outputWidth: Int
    let outputHeight: Int
    let confidence: Float?
    let areaRatio: Double?
    let topLeft: PointReport?
    let topRight: PointReport?
    let bottomRight: PointReport?
    let bottomLeft: PointReport?
    let error: String?
}

enum CropError: Error, CustomStringConvertible {
    case couldNotLoadImage(String)
    case noRectangleDetected
    case couldNotRenderImage
    case couldNotCreateBitmap
    case couldNotWriteImage(String)

    var description: String {
        switch self {
        case .couldNotLoadImage(let path):
            return "could not load image: \(path)"
        case .noRectangleDetected:
            return "no page rectangle detected"
        case .couldNotRenderImage:
            return "could not render corrected image"
        case .couldNotCreateBitmap:
            return "could not create analysis bitmap"
        case .couldNotWriteImage(let path):
            return "could not write image: \(path)"
        }
    }
}

func writeJson<T: Encodable>(_ value: T, to path: String?) throws {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    let data = try encoder.encode(value)

    if let path {
        try data.write(to: URL(fileURLWithPath: path))
    } else {
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([0x0a]))
    }
}

func imagePixelSize(_ path: String) -> (Int, Int) {
    let url = URL(fileURLWithPath: path)
    guard
        let source = CGImageSourceCreateWithURL(url as CFURL, nil),
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
    else {
        return (0, 0)
    }

    let width = properties[kCGImagePropertyPixelWidth] as? Int ?? 0
    let height = properties[kCGImagePropertyPixelHeight] as? Int ?? 0
    return (width, height)
}

func point(_ normalized: CGPoint, width: Double, height: Double) -> CGPoint {
    CGPoint(x: normalized.x * width, y: normalized.y * height)
}

func reportPoint(_ value: CGPoint) -> PointReport {
    PointReport(x: value.x, y: value.y)
}

func polygonArea(_ points: [CGPoint]) -> Double {
    guard points.count > 2 else {
        return 0
    }

    var total = 0.0
    for index in points.indices {
        let next = points[(index + 1) % points.count]
        total += points[index].x * next.y
        total -= next.x * points[index].y
    }

    return abs(total) / 2
}

func distance(_ a: CGPoint, _ b: CGPoint) -> Double {
    hypot(a.x - b.x, a.y - b.y)
}

func rectangleScore(_ observation: VNRectangleObservation, width: Double, height: Double) -> Double {
    let topLeft = point(observation.topLeft, width: width, height: height)
    let topRight = point(observation.topRight, width: width, height: height)
    let bottomRight = point(observation.bottomRight, width: width, height: height)
    let bottomLeft = point(observation.bottomLeft, width: width, height: height)
    let area = polygonArea([topLeft, topRight, bottomRight, bottomLeft])
    let areaRatio = area / (width * height)

    let topWidth = distance(topLeft, topRight)
    let bottomWidth = distance(bottomLeft, bottomRight)
    let leftHeight = distance(topLeft, bottomLeft)
    let rightHeight = distance(topRight, bottomRight)
    let averageWidth = (topWidth + bottomWidth) / 2
    let averageHeight = (leftHeight + rightHeight) / 2
    let aspectRatio = averageHeight > 0 ? averageWidth / averageHeight : 0

    let aspectScore = max(0.2, 1 - abs(aspectRatio - 0.72))
    return Double(observation.confidence) * areaRatio * aspectScore
}

struct AxisAlignedCrop {
    let rect: CGRect
    let areaRatio: Double
}

struct AnalysisBitmap {
    let width: Int
    let height: Int
    let pageLike: [Bool]
}

func bestRectangle(_ observations: [VNRectangleObservation], width: Double, height: Double) -> VNRectangleObservation? {
    observations
        .filter { observation in
            let points = [
                point(observation.topLeft, width: width, height: height),
                point(observation.topRight, width: width, height: height),
                point(observation.bottomRight, width: width, height: height),
                point(observation.bottomLeft, width: width, height: height),
            ]
            let areaRatio = polygonArea(points) / (width * height)
            return areaRatio >= 0.18 && areaRatio <= 0.98
        }
        .max { first, second in
            rectangleScore(first, width: width, height: height)
                < rectangleScore(second, width: width, height: height)
        }
}

func smoothed(_ scores: [Double], radius: Int) -> [Double] {
    guard radius > 0, !scores.isEmpty else {
        return scores
    }

    var prefix = [Double](repeating: 0, count: scores.count + 1)
    for index in scores.indices {
        prefix[index + 1] = prefix[index] + scores[index]
    }

    return scores.indices.map { index in
        let start = max(0, index - radius)
        let end = min(scores.count, index + radius + 1)
        return (prefix[end] - prefix[start]) / Double(end - start)
    }
}

func bestInterval(_ scores: [Double], threshold: Double, minimumLength: Int) -> Range<Int>? {
    var best: Range<Int>?
    var bestWeight = 0.0
    var start: Int?
    var total = 0.0

    for index in 0...scores.count {
        let active = index < scores.count && scores[index] >= threshold
        if active {
            if start == nil {
                start = index
                total = 0
            }
            total += scores[index]
            continue
        }

        guard let intervalStart = start else {
            continue
        }

        let length = index - intervalStart
        if length >= minimumLength, total > bestWeight {
            best = intervalStart..<index
            bestWeight = total
        }
        start = nil
        total = 0
    }

    return best
}

func analysisBitmap(from image: CIImage, maxDimension: CGFloat = 900) throws -> AnalysisBitmap {
    let extent = image.extent.integral
    let scale = min(1, maxDimension / max(extent.width, extent.height))
    let analysisImage = image.transformed(by: CGAffineTransform(scaleX: scale, y: scale))
    let context = CIContext()
    guard let cgImage = context.createCGImage(analysisImage, from: analysisImage.extent.integral) else {
        throw CropError.couldNotRenderImage
    }

    let width = cgImage.width
    let height = cgImage.height
    let bytesPerPixel = 4
    let bytesPerRow = width * bytesPerPixel
    let colorSpace = CGColorSpaceCreateDeviceRGB()
    var pixels = [UInt8](repeating: 0, count: height * bytesPerRow)

    try pixels.withUnsafeMutableBytes { rawBuffer in
        guard let bitmap = CGContext(
            data: rawBuffer.baseAddress,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: bytesPerRow,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else {
            throw CropError.couldNotCreateBitmap
        }
        bitmap.draw(cgImage, in: CGRect(x: 0, y: 0, width: width, height: height))
    }

    var pageLike = [Bool](repeating: false, count: width * height)
    for y in 0..<height {
        for x in 0..<width {
            let offset = y * bytesPerRow + x * bytesPerPixel
            let red = Double(pixels[offset])
            let green = Double(pixels[offset + 1])
            let blue = Double(pixels[offset + 2])
            let maxChannel = max(red, green, blue)
            let minChannel = min(red, green, blue)
            let brightness = (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255
            let saturation = maxChannel > 0 ? (maxChannel - minChannel) / maxChannel : 0

            pageLike[y * width + x] = brightness > 0.34 && saturation < 0.28
        }
    }

    return AnalysisBitmap(width: width, height: height, pageLike: pageLike)
}

func fallbackAxisAlignedCrop(_ image: CIImage) throws -> AxisAlignedCrop? {
    let bitmap = try analysisBitmap(from: image)
    let width = bitmap.width
    let height = bitmap.height

    var columnScores = [Double](repeating: 0, count: width)
    var rowScores = [Double](repeating: 0, count: height)

    for y in 0..<height {
        for x in 0..<width where bitmap.pageLike[y * width + x] {
            columnScores[x] += 1
            rowScores[y] += 1
        }
    }

    columnScores = smoothed(columnScores.map { $0 / Double(height) }, radius: max(3, width / 100))
    rowScores = smoothed(rowScores.map { $0 / Double(width) }, radius: max(3, height / 100))

    guard
        let xRange = bestInterval(columnScores, threshold: 0.42, minimumLength: width / 2),
        let yRange = bestInterval(rowScores, threshold: 0.42, minimumLength: height / 2)
    else {
        return nil
    }

    let scaleX = image.extent.width / Double(width)
    let scaleY = image.extent.height / Double(height)
    let leftPadding = xRange.lowerBound > width / 20 ? image.extent.width * 0.06 : image.extent.width * 0.012
    let rightPadding = image.extent.width * 0.012
    let topPadding = image.extent.height * 0.012
    let bottomPadding = image.extent.height * 0.06

    let minX = max(image.extent.minX, Double(xRange.lowerBound) * scaleX - leftPadding)
    let maxX = min(image.extent.maxX, Double(xRange.upperBound) * scaleX + rightPadding)
    let topY = max(0, Double(yRange.lowerBound) * scaleY - topPadding)
    let bottomY = min(image.extent.height, Double(yRange.upperBound) * scaleY + bottomPadding)
    let minY = max(image.extent.minY, image.extent.maxY - bottomY)
    let maxY = min(image.extent.maxY, image.extent.maxY - topY)
    let rect = CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    let areaRatio = (rect.width * rect.height) / (image.extent.width * image.extent.height)

    guard areaRatio >= 0.18, areaRatio <= 0.99 else {
        return nil
    }

    return AxisAlignedCrop(rect: rect, areaRatio: areaRatio)
}

func writePng(_ image: CIImage, to path: String) throws -> (Int, Int) {
    let context = CIContext()
    let extent = image.extent.integral
    guard let cgImage = context.createCGImage(image, from: extent) else {
        throw CropError.couldNotRenderImage
    }

    let url = URL(fileURLWithPath: path)
    guard let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else {
        throw CropError.couldNotWriteImage(path)
    }
    CGImageDestinationAddImage(destination, cgImage, nil)
    guard CGImageDestinationFinalize(destination) else {
        throw CropError.couldNotWriteImage(path)
    }

    return (cgImage.width, cgImage.height)
}

func cropPage(inputPath: String, outputPath: String, insetRatio: Double, metadataPath: String?) throws {
    let inputUrl = URL(fileURLWithPath: inputPath)
    guard let sourceImage = CIImage(contentsOf: inputUrl, options: [.applyOrientationProperty: true]) else {
        throw CropError.couldNotLoadImage(inputPath)
    }

    let extent = sourceImage.extent
    let width = extent.width
    let height = extent.height

    let request = VNDetectRectanglesRequest()
    request.maximumObservations = 12
    request.minimumConfidence = 0.35
    request.minimumSize = 0.18
    request.minimumAspectRatio = 0.25
    request.maximumAspectRatio = 1.15
    request.quadratureTolerance = 35

    let handler = VNImageRequestHandler(ciImage: sourceImage, options: [:])
    try handler.perform([request])

    if let rectangle = bestRectangle(request.results ?? [], width: width, height: height) {
        let topLeft = point(rectangle.topLeft, width: width, height: height)
        let topRight = point(rectangle.topRight, width: width, height: height)
        let bottomRight = point(rectangle.bottomRight, width: width, height: height)
        let bottomLeft = point(rectangle.bottomLeft, width: width, height: height)

        let corrected = sourceImage
            .applyingFilter(
                "CIPerspectiveCorrection",
                parameters: [
                    "inputTopLeft": CIVector(cgPoint: topLeft),
                    "inputTopRight": CIVector(cgPoint: topRight),
                    "inputBottomRight": CIVector(cgPoint: bottomRight),
                    "inputBottomLeft": CIVector(cgPoint: bottomLeft),
                ]
            )

        let correctedExtent = corrected.extent
        let dx = correctedExtent.width * insetRatio
        let dy = correctedExtent.height * insetRatio
        let finalImage = corrected.cropped(to: correctedExtent.insetBy(dx: dx, dy: dy))
        let outputSize = try writePng(finalImage, to: outputPath)

        let areaRatio = polygonArea([topLeft, topRight, bottomRight, bottomLeft]) / (width * height)
        try writeJson(
            CropReport(
                inputPath: inputPath,
                outputPath: outputPath,
                didCrop: true,
                method: "vision-rectangle",
                sourceWidth: Int(width.rounded()),
                sourceHeight: Int(height.rounded()),
                outputWidth: outputSize.0,
                outputHeight: outputSize.1,
                confidence: rectangle.confidence,
                areaRatio: areaRatio,
                topLeft: reportPoint(topLeft),
                topRight: reportPoint(topRight),
                bottomRight: reportPoint(bottomRight),
                bottomLeft: reportPoint(bottomLeft),
                error: nil
            ),
            to: metadataPath
        )
        return
    }

    guard let fallback = try fallbackAxisAlignedCrop(sourceImage) else {
        throw CropError.noRectangleDetected
    }

    let finalImage = sourceImage.cropped(to: fallback.rect)
    let outputSize = try writePng(finalImage, to: outputPath)
    let topLeft = CGPoint(x: fallback.rect.minX, y: fallback.rect.maxY)
    let topRight = CGPoint(x: fallback.rect.maxX, y: fallback.rect.maxY)
    let bottomRight = CGPoint(x: fallback.rect.maxX, y: fallback.rect.minY)
    let bottomLeft = CGPoint(x: fallback.rect.minX, y: fallback.rect.minY)

    try writeJson(
        CropReport(
            inputPath: inputPath,
            outputPath: outputPath,
            didCrop: true,
            method: "brightness-fallback",
            sourceWidth: Int(width.rounded()),
            sourceHeight: Int(height.rounded()),
            outputWidth: outputSize.0,
            outputHeight: outputSize.1,
            confidence: nil,
            areaRatio: fallback.areaRatio,
            topLeft: reportPoint(topLeft),
            topRight: reportPoint(topRight),
            bottomRight: reportPoint(bottomRight),
            bottomLeft: reportPoint(bottomLeft),
            error: nil
        ),
        to: metadataPath
    )
}

let arguments = Array(CommandLine.arguments.dropFirst())
if arguments.isEmpty {
    FileHandle.standardError.write(
        Data("usage: swift tools/ocr/page_crop.swift [--metadata <path>] [--inset <ratio>] <input-image> <output-png>\n".utf8)
    )
    exit(2)
}

var metadataPath: String?
var insetRatio = 0.006
var paths: [String] = []
var index = 0

while index < arguments.count {
    let argument = arguments[index]

    if argument == "--metadata" {
        guard index + 1 < arguments.count else {
            FileHandle.standardError.write(Data("missing value for --metadata\n".utf8))
            exit(2)
        }
        metadataPath = arguments[index + 1]
        index += 2
        continue
    }

    if argument == "--inset" {
        guard index + 1 < arguments.count, let value = Double(arguments[index + 1]) else {
            FileHandle.standardError.write(Data("invalid value for --inset\n".utf8))
            exit(2)
        }
        insetRatio = value
        index += 2
        continue
    }

    paths.append(argument)
    index += 1
}

guard paths.count == 2 else {
    FileHandle.standardError.write(Data("expected input and output paths\n".utf8))
    exit(2)
}

do {
    try cropPage(inputPath: paths[0], outputPath: paths[1], insetRatio: insetRatio, metadataPath: metadataPath)
} catch {
    let sourceSize = imagePixelSize(paths[0])
    let report = CropReport(
        inputPath: paths[0],
        outputPath: paths[1],
        didCrop: false,
        method: "none",
        sourceWidth: sourceSize.0,
        sourceHeight: sourceSize.1,
        outputWidth: 0,
        outputHeight: 0,
        confidence: nil,
        areaRatio: nil,
        topLeft: nil,
        topRight: nil,
        bottomRight: nil,
        bottomLeft: nil,
        error: String(describing: error)
    )
    try? writeJson(report, to: metadataPath)
    FileHandle.standardError.write(Data("\(error)\n".utf8))
    exit(4)
}
