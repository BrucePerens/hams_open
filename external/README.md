# External Dependencies

This module provides local hosting of external JavaScript and CSS libraries to support deployments in isolated or restricted networks. By serving these assets from the Odoo server itself, we eliminate dependencies on external Content Delivery Networks (CDNs), improving privacy, security, and reliability.

## Hosted Libraries

Each library's license below was verified against its own published `package.json` `license`
field on npm/unpkg (not assumed from memory) -- see `LICENSING.md` at the top of this repo for
the full picture of how this fits in with the rest of the codebase's licensing, and
`THIRD_PARTY_LICENSES.md` in this same directory for the actual reproduced copyright notice and
license text each library's own license requires, not just its SPDX identifier.

### Leaflet.js
- **Version:** 1.9.4
- **License:** BSD-2-Clause
- **Purpose:** Interactive maps for radio amateur applications.
- **Local Path:** `/external/static/src/node_modules/leaflet/`

### Transformers.js
- **Version:** 2.16.1 (published as `@xenova/transformers`)
- **License:** Apache-2.0
- **Purpose:** Machine Learning (NLP) at the edge for speech-to-text and entity extraction.
- **Local Path:** `/external/static/src/node_modules/transformers/transformers.js`

### D3.js, d3-geo-projection, and topojson-client
- **Version:** D3 v7.9.0, d3-geo-projection v4.0.0, topojson-client v3.1.0
- **License:** ISC (all three)
- **Purpose:** Geographic/map rendering support alongside Leaflet. `d3-geo-projection` supplies
  map projections D3's own core package doesn't ship.
- **Local Path:** `/external/static/src/node_modules/d3/d3.v7.min.js`,
  `/external/static/src/node_modules/d3/d3-geo-projection.v4.min.js`,
  `/external/static/src/node_modules/topojson/topojson-client.min.js`
- **`d3-geo-projection` was vendored on disk with no README entry until an earlier pass** -- same
  "real file, nothing documenting it" gap shape as other findings that night, just in documentation.
- **Resolved (2026-08-26):** the byte-level provenance gap is now understood precisely -- see
  `docs/proposals/VENDORED_ASSET_LICENSE_ATTRIBUTION.md`'s "Resolved: D3.js/topojson-client
  provenance" section for the full account, including why an earlier attempt to just replace these
  files with a naive fresh unpkg fetch broke 12 tests and was reverted. In short:
  `topojson-client.min.js` is byte-identical to a fresh unpkg download once the
  `/** @odoo-module **/` banner is stripped; `d3.v7.min.js` and `d3-geo-projection.v4.min.js` carry
  the same banner plus internal line-reflowing versus a fresh download (same tokens, different line
  breaks); `d3-geo-projection.v4.min.js` additionally has its UMD wrapper's `require("d3-geo")`/
  `require("d3-array")` calls replaced with nonexistent `importModule(...)` calls and its AMD branch
  disabled -- a targeted, deliberate-looking fix (only the one file with real external `require()`
  calls got this treatment) for what a fresh-download replacement reproduces as a real
  "d3-array module dependency" failure at test time.
  **Confirmed 2026-08-26, from Odoo's own `js_transpiler.py` source, not a hypothesis:**
  `convert_relative_require()`'s `RELATIVE_REQUIRE_RE` regex scans the *entire content* of every
  `@odoo-module`-tagged file for any `require("literal")` call -- relative or not -- and
  unconditionally registers each match as a real Odoo asset-bundle dependency
  (`dependencies.add(module_path)`), regardless of whether that call is reachable code. The UMD
  wrapper's CommonJS-detection branch (`"object"==typeof exports&&"undefined"!=typeof module?
  r(exports, require("d3-geo"), require("d3-array")):...`) never actually executes in a real
  browser (`typeof module` is never `"object"` there) -- but the transpiler's dependency scan runs
  at the text level, before/independent of any runtime branch logic, so it doesn't matter that the
  code is dead: it still sees the literal `require("d3-geo")`/`require("d3-array")` tokens and
  declares them as real dependencies this module needs Odoo's own registry to resolve. Since
  `d3-geo`/`d3-array` were never separately vendored as their own Odoo modules (only the bundled
  `d3.v7.min.js` exists), that declared-but-unresolvable dependency is exactly what produces the
  observed "d3-array module dependency" failure. Renaming the calls to `importModule(...)` (a name
  the regex doesn't match) removes them from the transpiler's dependency scan entirely -- the calls
  stay genuinely dead code either way, since the CommonJS branch still never executes; the rename
  only stops Odoo's text-level dependency scanner from falsely believing this module needs `d3-geo`/
  `d3-array` resolved. `fetch_assets.py` now has a real, hash-pinned,
  documented fetch+transform for all three (see below) -- but the *live* vendored files were
  deliberately left untouched rather than swapped for freshly-generated output, since they're
  proven-working in production and the earlier swap-without-full-verification is exactly what broke
  things. Anyone regenerating these should run the full test suite, not just the map-specific tests,
  before replacing the live files.

### Noble crypto libraries (@noble/curves, @noble/hashes, @noble/ciphers)
- **Version:** 2.3.0 (all three packages)
- **Purpose:** The primitive layer (X25519, SHA-256, HKDF, ChaCha20-Poly1305) underneath
  `hams_com`'s browser-side Noise_XX handshake (`ham_shack/static/src/js/noise_xx.js`,
  `docs/proposals/TRANSMITTER_HIJACK_PREVENTION.md` section 1). These used to be imported live from
  `cdn.jsdelivr.net` on every handshake -- a real, unmitigated CDN dependency for the exact code
  establishing a secure session, against this same self-hosting policy. Fixed by vendoring.
- **Local Path:** `/external/static/src/node_modules/noble/{curves/ed25519.js,hashes/sha2.js,
  hashes/hkdf.js,ciphers/chacha.js}`
- **Why bundled, not just copied:** the raw npm source uses bare-specifier internal imports (e.g.
  `"@noble/hashes/sha2.js"`) that a plain browser `import()` can't resolve without a bundler or
  import map. Each file below is an esbuild bundle with zero remaining external imports (verified by
  grep on each file after building).
- **Reproducible build:**
  ```bash
  npm install @noble/curves@2.3.0 @noble/hashes@2.3.0 @noble/ciphers@2.3.0
  npx esbuild@0.24.0 node_modules/@noble/curves/ed25519.js --bundle --format=esm \
      --outfile=external/static/src/node_modules/noble/curves/ed25519.js
  npx esbuild@0.24.0 node_modules/@noble/hashes/sha2.js --bundle --format=esm \
      --outfile=external/static/src/node_modules/noble/hashes/sha2.js
  npx esbuild@0.24.0 node_modules/@noble/hashes/hkdf.js --bundle --format=esm \
      --outfile=external/static/src/node_modules/noble/hashes/hkdf.js
  npx esbuild@0.24.0 node_modules/@noble/ciphers/chacha.js --bundle --format=esm \
      --outfile=external/static/src/node_modules/noble/ciphers/chacha.js
  ```
  (Prepend each output file's original documentation header back on, since esbuild overwrites the
  file.) SHA-256 of the currently-vendored files, for anyone re-verifying a future rebuild reproduces
  byte-identical output:
  ```
  d0f9264e701705e69636d2d255da124a1ab174002071442790e02081c1fc492c  curves/ed25519.js
  03c0e2ea7f737e6937217e5fbeb2875bb3af4444f2dc6ea838659c3af63d904f  hashes/sha2.js
  ae36283bf6c99ab940422e6c138ba4798b6aa77b9acae58f7eb8c70569798e2a  hashes/hkdf.js
  1484de9786fb7a9e595c71477612f992ae3203c30ff252b76179bf0031c35aad  ciphers/chacha.js
  ```
- **Verified** with `hams_com/ham_shack/tests/verify_noise_xx_handshake.py`: a real headless-Chromium
  Playwright run driving the real X25519/SHA-256/HKDF/ChaCha20-Poly1305 code paths through a full
  Noise_XX handshake, with network interception confirming zero requests leave localhost.

### ft8js (WASM FT8 decode + encode)
- **Version:** 0.0.3 upstream, but not vendored as the unmodified upstream build -- see below.
- **License:** MIT (both `ft8js`'s own wrapper code and the underlying `ft8_lib` it compiles, per
  each project's own `LICENSE`/`LICENSE-MIT` file, checked directly rather than assumed from the
  SPDX tag alone).
- **Purpose:** `SOFTWARE_ANALYSIS_PROPOSALS.md` item 1 -- real, client-side, in-browser FT8
  decoding for `ham_shack`, so a browser-based shack has native decode capability, not only a
  server-relayed one. `ham_shack`'s own `ft8_browser_decoder.js` only exposes decode (encode-side
  TX already exists server-side, `hams_open`'s own native decoder handles the relay-connected
  case) -- the encoder is vendored alongside the decoder anyway because it's the cleanest way to
  get a real, correctly-encoded FT8 test signal for the in-browser hoot test below (encode a real
  message, decode it back, in the actual headless-Chrome test browser) without committing a binary
  WAV fixture.
- **Local Path:** `/external/static/src/node_modules/ft8js/{decode.js,decode.wasm,encode.js,encode.wasm}`
- **Not a straight vendor of the npm package -- rebuilt from source, and why:** the published npm
  tarball's own `decode.js`/`decode.wasm` were compiled against an older `ft8_lib` commit whose
  `ftx_message_decode()` signature has since changed upstream (a 4th `offsets` parameter was
  added) -- the npm package's own bundled `src/decode.c` no longer compiles against a current
  `ft8_lib` checkout unmodified. Rebuilt directly from `ft8js`'s own published `src/decode.c`
  (one-line compatibility fix: pass `NULL` for the new `offsets` parameter, since this decoder
  doesn't use per-field offset reporting) against a full, current `ft8_lib` checkout, using
  `ft8js`'s own documented `emcc` build command from its `package.json` (`emscripten` 3.1.69,
  installed via `apt-get install emscripten`). Verified as a faithful rebuild, not a divergent
  fork: decoded identically to the npm package's own bundled build against the same real
  `ft8sim`-generated test signals before any further changes were considered.
- **A real algorithmic-improvement hypothesis was tested here and honestly falsified -- recorded
  so a future session doesn't re-spend the time re-testing it.** `SOFTWARE_ANALYSIS_PROPOSALS.md`
  records Bruce's direct instruction to study why the same author's `ft8ts` (a pure-TypeScript,
  GPL-3.0 port of WSJT-X's own reference LDPC decoder) claims a real, benchmarked accuracy
  advantage over `ft8_lib`/`ft8js` (17/N vs 8/N decoded messages on the author's own test set), and
  reimplement whatever real improvement is found as original, MIT-licensed code -- not port or
  copy `ft8ts`'s own GPL source. Reading both implementations side by side found one concrete,
  well-understood structural difference: `ft8_lib`'s `ldpc.c` uses `fast_tanh`/`fast_atanh`, a
  low-order rational (Padé-style) polynomial approximation in single-precision `float`, inside the
  LDPC belief-propagation message-passing loop; `ft8ts`'s `decode174_91.ts` (a direct port of
  WSJT-X's own `bpdecode174_91.f90`) uses JavaScript's native, exact `Math.tanh()`/`Math.log()`-based
  `atanh` in double precision throughout. Belief propagation is iterative and multiplicative across
  LDPC check-node edges (`kLDPC_iterations = 25`), so the reasoned hypothesis was that per-edge
  approximation/precision error compounds over those iterations and disproportionately hurts
  marginal, low-SNR decodes -- exactly the regime `ft8ts`'s benchmark claims an advantage in.
  **Tested it directly rather than trusting the reasoning**: patched a local `ldpc.c` to use exact
  `tanhf`/`atanhf` (real C99 libm functions, not copied from `ft8ts` or anywhere else -- this
  satisfies the "reimplement, don't port" requirement independent of the outcome) in place of the
  approximations, built both the patched and unpatched decoder to WASM, and ran both against 120
  identical real `ft8sim`-generated signals (`"CQ K6BP CM87"`, 20 trials each at -16/-18/-20/-21/
  -22/-23 dB) via a Node.js harness exercising the real WASM decode path. **Result: no improvement.**
  Baseline: 15/20 at -16dB, 15/20 at -18dB, 0/20 at -20dB and below. Exact-tanh patch: 15/20 at
  -16dB, 14/20 at -18dB (slightly *worse*, on byte-identical input), 0/20 at -20dB and below. The
  tanh/atanh precision is therefore not the real explanation for `ft8ts`'s claimed advantage --
  the actual cause remains genuinely unknown and is real, open, unscoped follow-on work (candidate
  Costas-sync search width/threshold, time/frequency oversampling ratio, or LDPC early-termination
  heuristic are the next places to look, not yet investigated). The shipped `decode.js`/`decode.wasm`
  above are the **unpatched, faithfully-rebuilt baseline** -- the tanh patch was not shipped, since
  it isn't a real improvement.
- **Reproducible build** (patched-copy testing artifacts were not committed; this reproduces the
  shipped, unpatched baseline):
  ```bash
  sudo apt-get install -y emscripten   # provides emcc 3.1.69
  git clone https://github.com/kgoba/ft8_lib.git   # or use hams_open's own vendored copy at
                                                     # daemons/ham_digital_modes/vendor/ft8_lib,
                                                     # which omits common/audio.c and common/wave.c
                                                     # (not needed by the native Rust FFI wrapper) --
                                                     # ft8js's own decode.c needs those two files, so
                                                     # a full upstream checkout is simpler here.
  npm pack ft8js@0.0.3 && tar xzf ft8js-0.0.3.tgz
  cp package/src/decode.c .
  # One-line compatibility fix for the current ftx_message_decode() signature:
  sed -i 's/ftx_message_decode(&message, &hash_if, text);/ftx_message_decode(\&message, \&hash_if, text, NULL);/' decode.c
  emcc -s EXPORT_NAME="'___ft8jsDecodeModule___'" -Ift8_lib -sSTACK_SIZE=5MB \
    decode.c ft8_lib/ft8/message.c ft8_lib/ft8/text.c ft8_lib/ft8/decode.c ft8_lib/ft8/encode.c \
    ft8_lib/ft8/constants.c ft8_lib/ft8/crc.c ft8_lib/ft8/ldpc.c ft8_lib/common/audio.c \
    ft8_lib/common/monitor.c ft8_lib/common/wave.c ft8_lib/fft/kiss_fft.c ft8_lib/fft/kiss_fftr.c \
    -o external/static/src/node_modules/ft8js/decode.js \
    -sEXPORTED_FUNCTIONS='["_init_decode", "_exec_decode", "_free", "_malloc"]' \
    -sEXPORTED_RUNTIME_METHODS=cwrap -s ASYNCIFY=1 -s 'ASYNCIFY_IMPORTS=["_exec_decode"]' \
    --no-entry -flto -s EXPORT_ES6=1 -s NO_FILESYSTEM=1 -s ALLOW_MEMORY_GROWTH=1 -s AUTO_NATIVE_LIBRARIES=0

  # Encoder needs no compatibility patch -- its C API didn't change upstream.
  cp package/src/encode.c .
  emcc -s EXPORT_NAME="'___ft8jsEncodeModule___'" -Ift8_lib -sSTACK_SIZE=2MB \
    encode.c ft8_lib/ft8/message.c ft8_lib/ft8/text.c ft8_lib/ft8/encode.c ft8_lib/ft8/constants.c ft8_lib/ft8/crc.c \
    -o external/static/src/node_modules/ft8js/encode.js \
    -sEXPORTED_FUNCTIONS='["_exec_encode", "_free", "_malloc"]' \
    -sEXPORTED_RUNTIME_METHODS=cwrap -s ASYNCIFY=1 -s 'ASYNCIFY_IMPORTS=["_exec_encode"]' \
    --no-entry -flto -s EXPORT_ES6=1 -s NO_FILESYSTEM=1 -s ALLOW_MEMORY_GROWTH=1 -s AUTO_NATIVE_LIBRARIES=0
  ```
  SHA-256 of the currently-vendored files:
  ```
  19b89dfb51c1ac56eda3c1844f34d6eb940bb3f9bba96dffe67411ed53dfc5ad  decode.js
  c1d8bfca917246030085c8abf5e71ee707f12dc49129f23d32174a126a209346  decode.wasm
  eaddfd9769986d006759fd6b1b155eb300b74e05ce904d878904b4d6c581f47c  encode.js
  5e7bce8a4346e521411f5066c955c3a24d134b7447c321f5db586f20d4895fa8  encode.wasm
  ```
- **Verified** two ways: (1) a Node.js harness (not committed -- scratch verification tooling)
  running the exact decode.wasm module against 120 real `ft8sim`-generated FT8 signals, decoding
  correctly at -16/-18dB and cleanly failing (not crashing, not hallucinating a message) at -20dB
  and below, plus a real encode-then-decode round trip through both WASM modules together
  (`"CQ K6BP CM87"` in, byte-identical text out); (2)
  `ham_shack/static/tests/ft8_browser_decode.test.js` -- a real hoot suite running that same
  encode-then-decode round trip inside the actual headless-Chrome test browser via a genuine
  `import()` of the vendored module (not a mock), proving the WASM-in-browser path specifically,
  not just WASM-in-Node.js.

## Maintenance

To update or refresh the local assets, the script `fetch_assets.py` can be executed. This script downloads the libraries directly into the module structure.

```bash
python3 external/fetch_assets.py
```

`main()` calls `download_file` (Leaflet, Transformers.js) directly; a separate, lower-level pair
handles the D3-family files, which need real post-download transformation (banner-prepending,
and for `d3-geo-projection`, the `require()`-neutering fix documented above), and which are hash-
verified against a pinned, currently-vendored-file hash rather than assumed correct:

- `hash_file(path)` -- SHA-256 of a file on disk, or `None` if it doesn't exist yet.
  [@ANCHOR: external:hash_file]
- `download_and_transform_file(url, dest_path, transform_fn, expected_hash)` -- like
  `download_file`, but applies `transform_fn(raw_bytes) -> bytes` to the download before hash-
  verifying and writing it, so the pinned hash covers the real, post-transform content that
  actually lands on disk. [@ANCHOR: external:download_and_transform_file]
- `_odoo_module_banner_transform`/`_d3_geo_projection_transform` are the two real transforms in
  current use, both documented in the D3.js section above.
  [@ANCHOR: external:_odoo_module_banner_transform] [@ANCHOR: external:_d3_geo_projection_transform]
- `fetch_d3_family_assets_INTENTIONALLY_NOT_CALLED_FROM_MAIN(lib_dir)` -- reproduces the D3-family
  vendoring end to end. Deliberately not wired into `main()`/called automatically -- see its own
  docstring for why (an earlier, reverted attempt at exactly that broke 12 unrelated tests). Run it
  deliberately, then run the full test suite (not just map-specific tests) before replacing the
  live vendored files with its output. [@ANCHOR: external:fetch_d3_family_assets]

## Usage in Other Modules

### Leaflet
Odoo's asset system will automatically include Leaflet in the backend and frontend bundles if this module is installed.

### Transformers.js
For modules using dynamic imports, use the local path:

```javascript
const module = await import('/external/static/src/node_modules/transformers/transformers.js');
```

## External Dependencies

- None
