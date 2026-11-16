import CryptoKit
import Foundation

/// A profile is a named cookie jar. WebKit identifies persistent data stores
/// by UUID, so the name is hashed into one: the same name always opens the
/// same store, and nothing has to be recorded on disk to find it again.
public enum Profile {
    public static let defaultName = "default"

    /// `nil` means the default store that tabs used before profiles existed.
    public static func storeIdentifier(for name: String?) -> UUID? {
        guard let name, !name.isEmpty, name != defaultName else { return nil }
        // The prefix is the project's former name. It must never change: a
        // different prefix gives different identifiers, and every profile's
        // logins would be left behind in stores nothing points to.
        let digest = SHA256.hash(data: Data(("weft-profile:" + name).utf8))
        var bytes = Array(digest.prefix(16))
        bytes[6] = (bytes[6] & 0x0F) | 0x80 // version 8: custom, name-derived
        bytes[8] = (bytes[8] & 0x3F) | 0x80 // RFC 4122 variant
        return UUID(uuid: (bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                           bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]))
    }
}
