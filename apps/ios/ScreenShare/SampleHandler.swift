// Songbird's screen-sharing extension (D-0180): ReplayKit hands it the screen once the person
// starts the broadcast from the system's sheet; it sends each frame, scaled to 1280 pixels on
// the long side and at most 15 a second, to the Songbird app over a socket in the app group's
// container, where only Songbird's own processes reach. The app puts it in the call, encrypted
// like the camera. Nothing is kept, and nothing goes anywhere else.
//
// The frame: "SBS1", width, height, rotation (UInt32, little-endian), then the NV12 luma rows
// and CbCr rows, packed.

import Accelerate
import CoreMedia
import Foundation
import ReplayKit

final class SampleHandler: RPBroadcastSampleHandler {
    private static let group = "group.chat.songbird.app"
    private static let maxSide = 1280.0
    private static let frameSeconds = 1.0 / 15

    private var fd: Int32 = -1
    private var last = CMTime.invalid
    private var luma = [UInt8]()
    private var chroma = [UInt8]()

    override func broadcastStarted(withSetupInfo setupInfo: [String: NSObject]?) {
        guard let dir = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: Self.group)
        else {
            return stop("Songbird can't share the screen on this device.")
        }
        let path = dir.appendingPathComponent("screen.sock").path
        fd = socket(AF_UNIX, SOCK_STREAM, 0)
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let bytes = Array(path.utf8)
        guard bytes.count < MemoryLayout.size(ofValue: addr.sun_path) else {
            return stop("Songbird can't share the screen on this device.")
        }
        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            raw.copyBytes(from: bytes)
            raw[bytes.count] = 0
        }
        let connected = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        if connected != 0 {
            stop("Start sharing from a call in Songbird.")
        }
    }

    override func processSampleBuffer(_ sample: CMSampleBuffer, with type: RPSampleBufferType) {
        guard type == .video, fd >= 0, let image = CMSampleBufferGetImageBuffer(sample) else {
            return
        }
        let time = CMSampleBufferGetPresentationTimeStamp(sample)
        if last.isValid, CMTimeGetSeconds(CMTimeSubtract(time, last)) < Self.frameSeconds {
            return
        }
        last = time
        let format = CVPixelBufferGetPixelFormatType(image)
        guard format == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            || format == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        else { return }

        CVPixelBufferLockBaseAddress(image, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(image, .readOnly) }
        let width = CVPixelBufferGetWidthOfPlane(image, 0)
        let height = CVPixelBufferGetHeightOfPlane(image, 0)
        let scale = min(1, Self.maxSide / Double(max(width, height)))
        let w = max(2, Int(Double(width) * scale) & ~1)
        let h = max(2, Int(Double(height) * scale) & ~1)
        guard let y0 = CVPixelBufferGetBaseAddressOfPlane(image, 0),
              let uv0 = CVPixelBufferGetBaseAddressOfPlane(image, 1)
        else { return }
        if luma.count != w * h {
            luma = [UInt8](repeating: 0, count: w * h)
            chroma = [UInt8](repeating: 0, count: w * h / 2)
        }
        var srcY = vImage_Buffer(data: y0, height: vImagePixelCount(height),
                                 width: vImagePixelCount(width),
                                 rowBytes: CVPixelBufferGetBytesPerRowOfPlane(image, 0))
        var srcUV = vImage_Buffer(data: uv0, height: vImagePixelCount(height / 2),
                                  width: vImagePixelCount(width / 2),
                                  rowBytes: CVPixelBufferGetBytesPerRowOfPlane(image, 1))
        let ok = luma.withUnsafeMutableBytes { y -> Bool in
            chroma.withUnsafeMutableBytes { uv -> Bool in
                var dstY = vImage_Buffer(data: y.baseAddress, height: vImagePixelCount(h),
                                         width: vImagePixelCount(w), rowBytes: w)
                var dstUV = vImage_Buffer(data: uv.baseAddress, height: vImagePixelCount(h / 2),
                                          width: vImagePixelCount(w / 2), rowBytes: w)
                return vImageScale_Planar8(&srcY, &dstY, nil, vImage_Flags(kvImageNoFlags)) == kvImageNoError
                    && vImageScale_CbCr8(&srcUV, &dstUV, nil, vImage_Flags(kvImageNoFlags)) == kvImageNoError
            }
        }
        guard ok else { return }

        var header = [UInt8]("SBS1".utf8)
        for v in [UInt32(w), UInt32(h), rotation(sample)] {
            withUnsafeBytes(of: v.littleEndian) { header.append(contentsOf: $0) }
        }
        if !send(header) || !send(luma) || !send(chroma) {
            stop("You stopped sharing in Songbird.")
        }
    }

    override func broadcastFinished() {
        if fd >= 0 { close(fd) }
        fd = -1
    }

    /// Clockwise degrees to turn the frame upright, from ReplayKit's orientation.
    private func rotation(_ sample: CMSampleBuffer) -> UInt32 {
        guard let value = CMGetAttachment(sample, key: RPVideoSampleOrientationKey as CFString,
                                          attachmentModeOut: nil) as? NSNumber
        else { return 0 }
        switch CGImagePropertyOrientation(rawValue: value.uint32Value) {
        case .right: return 90
        case .down: return 180
        case .left: return 270
        default: return 0
        }
    }

    private func send(_ bytes: [UInt8]) -> Bool {
        var sent = 0
        while sent < bytes.count {
            let n = bytes.withUnsafeBytes { write(fd, $0.baseAddress! + sent, bytes.count - sent) }
            if n <= 0 { return false }
            sent += n
        }
        return true
    }

    private func stop(_ reason: String) {
        if fd >= 0 { close(fd) }
        fd = -1
        finishBroadcastWithError(NSError(domain: "chat.songbird.screen", code: 1,
                                         userInfo: [NSLocalizedDescriptionKey: reason]))
    }
}
