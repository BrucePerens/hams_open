# Story: Local Asset Hosting

To support deployments in isolated or restricted networks, the platform must host critical external libraries locally.

## Leaflet.js
The mapping library Leaflet.js is hosted in the `external` module.
- [@ANCHOR: external:HTTP_REACHABLE_LEAFLET]

- [@ANCHOR: external:HTTP_NO_HEAD]

- [@ANCHOR: external:HTTP_NO_MASKING]

## Transformers.js
The machine learning library Transformers.js is hosted in the `external` module to support Edge AI features like callsign recognition.
- [@ANCHOR: external:HTTP_REACHABLE_TRANSFORMERS]

- [@ANCHOR: external:TRANSFORMERS_MIN]

## ft8js
(WASM is WebAssembly, the compiled binary format browsers run alongside JavaScript; FT8 is a weak-signal amateur-radio digital mode. See the ft8js section of [the module README](../../README.md).)

The vendored WASM FT8 decode/encode modules are hosted in the `external` module so a browser can decode/encode real FT8 signals client-side without a network round-trip to any daemon.
- [@ANCHOR: external:HTTP_REACHABLE_FT8JS]

## SIP.js
The browser SIP stack SIP.js 0.21.2 (MIT licence, ADR 0101's browser-direct AllStar client) is hosted in the `external` module as plain ES modules under `static/src/node_modules/sipjs/`, so a browser can register with and call an AllStar node itself with no hams.com server in the signalling or audio path. The files are the npm tarball's own, unmodified, and `sipjs/SHA256SUMS` records the SHA-256 of each one. The directory is named `sipjs`, not `sip.js`, because Odoo's asset pre-generation treats any path ending in `.js` as a file. The test fetches the library entry point through the web server, then fetches every listed file and compares its digest with the recorded one, so a damaged or tampered copy fails.
- [@ANCHOR: external:HTTP_REACHABLE_SIPJS]

