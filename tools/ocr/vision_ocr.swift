import CoreGraphics
import Foundation
import ImageIO
import Vision

struct ImageReport: Encodable {
    let path: String
    let width: Int
    let height: Int
    let observationCount: Int
    let characterCount: Int
    let averageConfidence: Float
    let minimumConfidence: Float
    let sampleText: String?
    let text: String?
    let error: String?
}

func jsonLine<T: Encodable>(_ value: T, to handle: FileHandle = .standardOutput) throws {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    let data = try encoder.encode(value)
    handle.write(data)
    handle.write(Data([0x0a]))
}

func imageSize(_ url: URL) -> (Int, Int)? {
    guard
        let source = CGImageSourceCreateWithURL(url as CFURL, nil),
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
        let width = properties[kCGImagePropertyPixelWidth] as? Int,
        let height = properties[kCGImagePropertyPixelHeight] as? Int
    else {
        return nil
    }
    return (width, height)
}

func normalizeWhitespace(_ value: String) -> String {
    value
        .split { $0.isWhitespace }
        .joined(separator: " ")
}

func performTextRecognition(url: URL, languages: [String]?) throws -> [VNRecognizedTextObservation] {
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = true
    if let languages {
        request.recognitionLanguages = languages
    }

    let handler = VNImageRequestHandler(url: url, options: [:])
    try handler.perform([request])
    return request.results ?? []
}

func recognize(_ path: String, includeSampleText: Bool, includeFullText: Bool) -> ImageReport {
    let url = URL(fileURLWithPath: path)
    let size = imageSize(url) ?? (0, 0)

    let languageAttempts: [[String]?] = [
        ["en-US", "ko-KR"],
        ["en-US"],
        nil,
    ]

    do {
        var observations: [VNRecognizedTextObservation] = []
        var lastError: Error?
        for languages in languageAttempts {
            do {
                observations = try performTextRecognition(url: url, languages: languages)
                lastError = nil
                break
            } catch {
                lastError = error
            }
        }
        if let lastError {
            throw lastError
        }
        let candidates = observations.compactMap { $0.topCandidates(1).first }
        let text = candidates.map(\.string)
        let confidences = candidates.map(\.confidence)
        let confidenceTotal = confidences.reduce(Float(0), +)
        let averageConfidence = confidences.isEmpty ? 0 : confidenceTotal / Float(confidences.count)
        let minimumConfidence = confidences.min() ?? 0
        let sampleText = includeSampleText ? normalizeWhitespace(text.joined(separator: " ")) : ""

        return ImageReport(
            path: path,
            width: size.0,
            height: size.1,
            observationCount: observations.count,
            characterCount: text.reduce(0) { $0 + $1.count },
            averageConfidence: averageConfidence,
            minimumConfidence: minimumConfidence,
            sampleText: includeSampleText ? String(sampleText.prefix(600)) : nil,
            text: includeFullText ? text.joined(separator: "\n") : nil,
            error: nil
        )
    } catch {
        return ImageReport(
            path: path,
            width: size.0,
            height: size.1,
            observationCount: 0,
            characterCount: 0,
            averageConfidence: 0,
            minimumConfidence: 0,
            sampleText: includeSampleText ? "" : nil,
            text: includeFullText ? "" : nil,
            error: String(describing: error)
        )
    }
}

let arguments = Array(CommandLine.arguments.dropFirst())
if arguments.isEmpty {
    FileHandle.standardError.write(Data("usage: swift tools/ocr/vision_ocr.swift [--include-sample-text] [--include-full-text] [--output <path>] <image> [...]\n".utf8))
    exit(2)
}

var includeSampleText = false
var includeFullText = false
var outputPath: String?
var paths: [String] = []
var index = 0

while index < arguments.count {
    let argument = arguments[index]

    if argument == "--include-sample-text" {
        includeSampleText = true
        index += 1
        continue
    }

    if argument == "--include-full-text" {
        includeFullText = true
        index += 1
        continue
    }

    if argument == "--output" {
        guard index + 1 < arguments.count else {
            FileHandle.standardError.write(Data("missing value for --output\n".utf8))
            exit(2)
        }

        outputPath = arguments[index + 1]
        index += 2
        continue
    }

    paths.append(argument)
    index += 1
}

if paths.isEmpty {
    FileHandle.standardError.write(Data("at least one image path is required\n".utf8))
    exit(2)
}

let outputHandle: FileHandle
if let outputPath {
    FileManager.default.createFile(atPath: outputPath, contents: nil)
    outputHandle = try FileHandle(forWritingTo: URL(fileURLWithPath: outputPath))
} else {
    outputHandle = .standardOutput
}

for path in paths {
    try jsonLine(
        recognize(path, includeSampleText: includeSampleText, includeFullText: includeFullText),
        to: outputHandle
    )
}

if outputPath != nil {
    try outputHandle.close()
}
