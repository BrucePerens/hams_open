# SIIT mapping of IPv4 spaces into the translation block

A gateway operator maps one IPv4 address space (for example an AREDN mesh using 10/8) into IPv6 without state.
Each space is a domain with its own 16-bit ID, so two meshes that both use 10/8 never collide.

- [@ANCHOR: siit_map:embed] Host 10.1.2.3 in domain 7 becomes `<translation /48>:7::a01:203` (RFC 6052 /96 format); the 64:ff9b::/96 prefix is refused.
- [@ANCHOR: siit_map:extract] The inverse recovers the domain and the IPv4 address, and rejects addresses outside the block.
- [@ANCHOR: siit_map:domain_prefix] The /96 a domain announces by BGP.
- [@ANCHOR: siit_map:block] The translation /48 is a parameter (the /40 is not issued yet) and must be an IPv6 /48.
- [@ANCHOR: siit_map:domain] Domain IDs are 0 to 65535, assigned only by the registry.
- [@ANCHOR: siit_map:next_free_domain] Proposes the lowest unused ID to record in the registry.
- [@ANCHOR: siit_map:jool_siit_config] Generates the Jool SIIT instance file for one domain, from the registry's block and ID.
