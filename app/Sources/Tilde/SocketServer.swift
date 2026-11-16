import Foundation

/// One accepted client. Lines are delivered on the main queue.
final class Connection {
    let fd: Int32
    private let writeLock = NSLock()
    private var closed = false

    init(fd: Int32) { self.fd = fd }

    func send(_ line: String) {
        writeLock.lock(); defer { writeLock.unlock() }
        if closed { return }
        var bytes = Array(line.utf8)
        bytes.append(0x0A)
        var offset = 0
        while offset < bytes.count {
            let n = bytes[offset...].withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }
            if n <= 0 {
                if n < 0 && errno == EINTR { continue }
                return
            }
            offset += n
        }
    }

    func close() {
        writeLock.lock(); defer { writeLock.unlock() }
        if closed { return }
        closed = true
        shutdown(fd, SHUT_RDWR)
        Darwin.close(fd)
    }
}

/// Unix-domain socket server speaking newline-delimited UTF-8.
final class SocketServer {
    let path: String
    private var listenFD: Int32 = -1

    var onConnect: ((Connection) -> Void)?
    var onLine: ((Connection, String) -> Void)?
    var onDisconnect: ((Connection) -> Void)?

    init(path: String) { self.path = path }

    func start() throws {
        unlink(path)
        listenFD = socket(AF_UNIX, SOCK_STREAM, 0)
        guard listenFD >= 0 else { throw POSIXError(.init(rawValue: errno) ?? .EIO) }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(path.utf8)
        let capacity = MemoryLayout.size(ofValue: addr.sun_path)
        guard pathBytes.count < capacity else { throw POSIXError(.ENAMETOOLONG) }
        withUnsafeMutableBytes(of: &addr.sun_path) { raw in
            raw.copyBytes(from: pathBytes)
        }
        let len = socklen_t(MemoryLayout<sockaddr_un>.size)
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(listenFD, $0, len) }
        }
        guard bound == 0 else { throw POSIXError(.init(rawValue: errno) ?? .EIO) }
        chmod(path, 0o600)
        guard listen(listenFD, 4) == 0 else { throw POSIXError(.init(rawValue: errno) ?? .EIO) }

        let fd = listenFD
        Thread.detachNewThread { [weak self] in
            while true {
                let client = accept(fd, nil, nil)
                if client < 0 {
                    if errno == EINTR { continue }
                    return
                }
                var on: Int32 = 1
                setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
                let conn = Connection(fd: client)
                DispatchQueue.main.async { self?.onConnect?(conn) }
                Thread.detachNewThread { self?.readLoop(conn) }
            }
        }
    }

    private func readLoop(_ conn: Connection) {
        var pending = [UInt8]()
        var buffer = [UInt8](repeating: 0, count: 16 * 1024)
        while true {
            let n = buffer.withUnsafeMutableBytes { read(conn.fd, $0.baseAddress, $0.count) }
            if n < 0 && errno == EINTR { continue }
            if n <= 0 { break }
            pending.append(contentsOf: buffer[0..<n])
            while let nl = pending.firstIndex(of: 0x0A) {
                let lineBytes = pending[0..<nl]
                let line = String(decoding: lineBytes, as: UTF8.self)
                pending.removeSubrange(0...nl)
                if line.isEmpty { continue }
                DispatchQueue.main.async { [weak self] in self?.onLine?(conn, line) }
            }
        }
        DispatchQueue.main.async { [weak self] in
            self?.onDisconnect?(conn)
            conn.close()
        }
    }

    func stop() {
        if listenFD >= 0 {
            Darwin.close(listenFD)
            listenFD = -1
        }
        unlink(path)
    }
}
