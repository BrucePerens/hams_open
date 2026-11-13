# Story: Local Asset Hosting

To support deployments in isolated or restricted networks, the platform must host critical external libraries locally.

## Leaflet.js
The mapping library Leaflet.js is hosted in the `external` module.
- [@ANCHOR: external:HTTP_REACHABLE_LEAFLET] Leaflet 1.9.4 is served from this site. `test_01_leaflet_assets_reachable` in `tests/test_assets.py` fetches `/external/static/src/node_modules/leaflet/leaflet.js` and `leaflet.css` through the Odoo web server, asserts HTTP 200 and that the bodies contain `Leaflet` and `.leaflet-container`, so an empty or missing vendored file fails. The test needs the installed module and changes nothing.

- [@ANCHOR: external:HTTP_NO_HEAD] `fetch_assets.download_file(url, dest_path, expected_hash)` fetches a pinned asset with exactly one `GET` through `urllib.request.urlopen` (10 second timeout) and never a preliminary `HEAD`, streaming 8192-byte chunks to a temporary file while hashing. The test `test_03_no_head_request` patches `urlopen` and asserts one call; `test_02d_jspdf_and_fabric_reachable` also carries this anchor and checks jsPDF and fabric.js are served with HTTP 200.

- [@ANCHOR: external:HTTP_NO_MASKING] When a download fails after the temporary file was made, `download_file` removes the temporary file only if it still exists, then re-raises the original exception (`URLError`, `ValueError` for a hash mismatch, or `OSError`). The cleanup therefore never masks the real error with a `FileNotFoundError`. `test_04_exception_masking` makes `shutil.move` raise `OSError` with the temp file absent and asserts the original message surfaces.

## Transformers.js
The machine learning library Transformers.js is hosted in the `external` module to support Edge AI features like callsign recognition.
- [@ANCHOR: external:HTTP_REACHABLE_TRANSFORMERS] Transformers.js 2.16.1 is served from this site as `/external/static/src/node_modules/transformers/transformers.js`. `test_02_transformers_assets_reachable` fetches it through the web server and asserts HTTP 200 and that the body contains `transformers`, proving the vendored copy is present and not an empty placeholder. It changes nothing and needs the installed module.

- [@ANCHOR: external:TRANSFORMERS_MIN] `fetch_assets.main` fetches the minified build, `https://cdn.jsdelivr.net/npm/@xenova/transformers@2.16.1/dist/transformers.min.js`, pinned by SHA-256, and stores it as `transformers.js`; the minified file is used to avoid dependency-audit findings from the unminified one. `test_05_transformers_min_js` patches `download_file`, runs `main()` and asserts the Transformers URL ends in `transformers.min.js`.

## ft8js
(WASM is WebAssembly, the compiled binary format browsers run alongside JavaScript; FT8 is a weak-signal amateur-radio digital mode. See the ft8js section of [the module README](../../README.md).)

The vendored WASM FT8 (a weak-signal digital mode) decode/encode modules are hosted in the `external` module so a browser can decode/encode real FT8 signals client-side without a network round-trip to any daemon.
- [@ANCHOR: external:HTTP_REACHABLE_FT8JS] `test_02b_ft8js_decoder_reachable` fetches the vendored FT8 `decode.js` and `encode.js` through the web server and asserts HTTP 200 and the Emscripten module names `___ft8jsDecodeModule___` and `___ft8jsEncodeModule___`, then fetches `decode.wasm` and `encode.wasm` and asserts the real WebAssembly magic bytes `\x00asm`, so a browser can decode and encode FT8 locally. It needs the installed module and changes nothing; the vendored files are listed in `VENDORED.md`.

## SIP.js
The browser SIP (Session Initiation Protocol) stack SIP.js 0.21.2 (MIT licence, ADR 0101's browser-direct AllStar client) is hosted in the `external` module as plain ES modules under `static/src/node_modules/sipjs/`, so a browser can register with and call an AllStar node itself with no hams.com server in the signalling or audio path. The files are the npm tarball's own, unmodified, and `sipjs/SHA256SUMS` records the SHA-256 of each one. The directory is named `sipjs`, not `sip.js`, because Odoo's asset pre-generation treats any path ending in `.js` as a file. The test fetches the library entry point through the web server, then fetches every listed file and compares its digest with the recorded one, so a damaged or tampered copy fails.
- [@ANCHOR: external:HTTP_REACHABLE_SIPJS] `test_02c_sipjs_reachable_and_matches_the_recorded_checksums` (in `tests/test_assets.py`) fetches the SIP.js entry point through the web server, then every file listed in `sipjs/SHA256SUMS`, and compares each SHA-256 digest with the recorded one; it also checks `package.json` reports version 0.21.2 and `LICENSE.md` names the copyright holder, so a damaged or tampered copy fails. No arguments; read-only.

