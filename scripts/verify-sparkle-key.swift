// Match the signing seed to the public key embedded in the candidate.
// Read the private seed from stdin; never put it in arguments or output.
import CryptoKit
import Foundation
let encoded = String(decoding: FileHandle.standardInput.readDataToEndOfFile(), as: UTF8.self)
    .trimmingCharacters(in: .whitespacesAndNewlines)
guard CommandLine.arguments.count == 2,
      let seed = Data(base64Encoded: encoded), seed.count == 32 else {
    fputs("Expected a Sparkle seed and public key.\n", stderr)
    exit(1)
}
let key = try Curve25519.Signing.PrivateKey(rawRepresentation: seed)
guard key.publicKey.rawRepresentation.base64EncodedString() == CommandLine.arguments[1] else {
    fputs("Sparkle signing and public keys do not match.\n", stderr)
    exit(1)
}
