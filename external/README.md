# External Dependencies

This module provides local hosting of external JavaScript and CSS libraries to support deployments in isolated or restricted networks. By serving these assets from the Odoo server itself, we eliminate dependencies on external Content Delivery Networks (CDNs), improving privacy, security, and reliability.

## Hosted Libraries

### Leaflet.js
- **Version:** 1.9.4
- **Purpose:** Interactive maps for radio amateur applications.
- **Local Path:** `/external/static/src/node_modules/leaflet/`

### Transformers.js
- **Version:** 2.16.1
- **Purpose:** Machine Learning (NLP) at the edge for speech-to-text and entity extraction.
- **Local Path:** `/external/static/src/node_modules/transformers/transformers.js`

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

## Maintenance

To update or refresh the local assets, the script `fetch_assets.py` can be executed. This script downloads the libraries directly into the module structure.

```bash
python3 external/fetch_assets.py
```

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
