# Third-Party Notices

This file satisfies the attribution requirements of the permissive licenses under which the
JavaScript libraries in `external/static/src/node_modules/` are vendored (see `README.md` in this
directory for what each library is used for, and `LICENSING.md` at the top of this repo for how
this fits into the codebase's overall licensing picture). A bare SPDX identifier is not the same
thing as satisfying a license's own attribution clause -- this file reproduces the actual copyright
notice and license text for each vendored library, pulled from that library's own source file, not
assumed from memory.

---

## Leaflet.js 1.9.4 -- BSD-2-Clause

Copyright (c) 2010-2023, Vladimir Agafonkin
Copyright (c) 2010-2011, CloudMade

Redistribution and use in source and binary forms, with or without modification, are permitted
provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions
   and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of
   conditions and the following disclaimer in the documentation and/or other materials provided
   with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

---

## D3.js 7.9.0 -- ISC

Copyright 2010-2023 Mike Bostock

Permission to use, copy, modify, and/or distribute this software for any purpose with or without
fee is hereby granted, provided that the above copyright notice and this permission notice appear
in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS
SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE
AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE
OF THIS SOFTWARE.

## d3-geo-projection 4.0.0 -- ISC

Copyright 2013-2021 Mike Bostock, 2015 Ricky Reusser

Same ISC permission/warranty text as D3.js above.

Sourced via `fetch_assets.py`'s hash-pinned fetch (see the provenance note below for why this
replaced an earlier, undocumented, non-reproducible vendoring of the same library).

## topojson-client 3.1.0 -- ISC

Copyright 2019 Mike Bostock

Same ISC permission/warranty text as D3.js above.

---

## @noble/curves, @noble/hashes, @noble/ciphers 2.3.0 -- MIT

Copyright (c) Paul Miller (https://paulmillr.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge, publish, distribute,
sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT
OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

(Each vendored file already carries this same notice in its own header comment -- reproduced here
too so this file is a single, complete stop for the whole `external/` tree.)

---

## Transformers.js 2.16.1 (published as `@xenova/transformers`) -- Apache-2.0

Copyright 2023 The HuggingFace Team / Xenova

Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except
in compliance with the License. You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software distributed under the License
is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
implied. See the License for the specific language governing permissions and limitations under the
License.

**Known gap, found while writing this file, not previously documented:** the vendored
`transformers.js` is a ~1.7MB webpack bundle, and grepping it directly for `Copyright` strings
turns up `Copyright (c) Microsoft Corporation. All rights reserved.` repeated throughout -- almost
certainly from `onnxruntime-web` (MIT-licensed, the ONNX runtime Transformers.js bundles for
in-browser model inference), not from Xenova's own code. This means the single Apache-2.0 line
above is necessarily incomplete: a webpack bundle this size likely rolls in several third-party
packages' own licenses (onnxruntime-web's MIT terms at minimum), and none of those nested packages'
own attribution requirements have been individually verified or reproduced here. A real fix would
mean either building Transformers.js from source with a license-aggregation tool (e.g.
`license-checker` or webpack's own `LicenseWebpackPlugin`) to enumerate every bundled package, or
obtaining an official third-party-notices file from the upstream Transformers.js project if one
ships with a release. Not done tonight -- flagged here rather than presenting the single
Apache-2.0 line as if it were the complete picture.

---

## Provenance note: D3.js / d3-geo-projection / topojson-client (resolved)

All three D3-family files previously carried a shared `/** @odoo-module **/` banner line as their
first line -- Odoo's own asset-pipeline marker for a file importable as an ES module in Odoo's
frontend build -- which `fetch_assets.py`'s plain download-and-hash logic for Leaflet and
Transformers.js never added, so these three had come from some other, undocumented vendoring
process. One plausible source was checked and ruled out: Odoo core itself ships `topojson`-format
*data* files (continent/country boundaries) under `odoo/addons/spreadsheet/static/topojson/`, for
its Spreadsheet dashboard geo-chart feature, and that feature's chart library
(`odoo/addons/spreadsheet/static/lib/chartjs-chart-geo/chartjs-chart-geo.js`) was checked directly
for a bundled copy of D3/topojson-client source -- it contains at most one incidental substring
match, not a real embedded copy, so this was not where these three files came from.

**Resolved:** a search of this codebase found no manifest, XML asset bundle, or JS `import`
referencing any of the three files anywhere outside their own vendored directory -- they were dead,
unwired vendored code, never actually loaded by the running application, which is why the spurious
`/** @odoo-module **/` banner (and the non-standard `require(...)`-based fallback branch it implies
Odoo's real ES-module loader would need, which Odoo's own `module_loader.js` does not define) never
caused a runtime failure. Replaced with a fresh, hash-pinned fetch of the same published versions'
plain global-UMD `dist/` builds from unpkg (no `/** @odoo-module **/` banner, matching Leaflet's own
vendored form), now wired into `fetch_assets.py` identically to Leaflet and Transformers.js above.
`d3-geo-projection`'s browser-global UMD branch extends the same global `d3` object `d3.v7.min.js`
establishes, rather than depending on Odoo's module system at all -- the standard way D3 plugins are
loaded outside a bundler.
