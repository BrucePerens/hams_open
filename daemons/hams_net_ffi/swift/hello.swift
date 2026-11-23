// Hello-call test for the generated Swift bindings: SIIT map round trip and a refused address.
// Built and run by tools/swift_bindings.sh on macOS (swiftc, no Xcode needed).
@main
struct Hello {
    static func main() {
        do {
            let v6 = try siitEmbed(block: "2001:db8:1::/48", domain: 7, ipv4: "10.1.2.3")
            precondition(v6 == "2001:db8:1:7::a01:203", "embed gave \(v6)")
            let back = try siitExtract(block: "2001:db8:1::/48", address: v6)
            precondition(back.domain == 7 && back.ipv4 == "10.1.2.3", "extract mismatch")
        } catch {
            preconditionFailure("unexpected error \(error)")
        }
        do {
            _ = try siitEmbed(block: "2001:db8:1::/48", domain: 7, ipv4: "not-an-address")
            preconditionFailure("bad address was accepted")
        } catch is NetError {
            print("swift hello ok")
        } catch {
            preconditionFailure("wrong error type \(error)")
        }
    }
}
