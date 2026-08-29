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

**Not previously documented in `README.md`'s library list -- fixed as part of this pass.** This
file (`static/src/node_modules/d3/d3-geo-projection.v4.min.js`) is real, currently-vendored code
(used for projections D3's own core doesn't ship, e.g. for the map rendering alongside Leaflet)
that existed on disk with no corresponding README entry until now -- the same "real thing on disk,
nothing pointing at it" gap shape as tonight's other findings, just in documentation rather than
code.

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

**Nested bundled packages, resolved 2026-08-29 (previously an open gap).** The single Apache-2.0
line above covers Xenova's own code but not everything the ~1.7MB webpack bundle actually contains.
Rather than the full `license-checker`/`LicenseWebpackPlugin` rebuild-from-source this gap
originally called for (still not done -- no build toolchain for this vendored copy exists in this
repo), the bundle's own webpack module-boundary comments (`/*! <package-name> */`, immediately
preceding each `__webpack_require__` call) were read directly to identify exactly which named
packages are genuinely bundled with real code, as opposed to referenced-but-stubbed for a
browser build. Six candidates surfaced this way (`@huggingface/jinja`, `onnxruntime-common`,
`onnxruntime-node`, `onnxruntime-web`, plus Node built-ins `fs`/`path`/`url`/`stream/web`, and
`sharp`); confirmed by their resolved module paths that `onnxruntime-node`, `sharp`, and the four
Node built-ins each resolve to a bare `"?xxxx"` placeholder id (webpack's marker for an external,
Node-only dependency it could not bundle for a browser target), so none of those six actually ship
real code in this file -- only `@huggingface/jinja`, `onnxruntime-common`, and `onnxruntime-web`
resolve to real bundled module paths (`./node_modules/@huggingface/jinja/dist/index.js`,
`./node_modules/onnxruntime-common/dist/lib/index.js`,
`./node_modules/onnxruntime-web/dist/ort-web.min.js`) and need attribution here:

### `@huggingface/jinja` -- MIT

Copyright (c) 2023 Hugging Face

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

### `onnxruntime-common` and `onnxruntime-web` -- MIT

Copyright (c) Microsoft Corporation

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

(Copyright holder and license confirmed directly against each project's own upstream LICENSE file,
not assumed from the bundle's embedded `Copyright (c) Microsoft Corporation. All rights reserved.`
string alone; the huggingface.js monorepo's own top-level `LICENSE` file is `@huggingface/jinja`'s
actual license source, since that package ships no separate one of its own.)

**Second pass, same evening: `onnxruntime-web`'s own transitive dependencies.** The gap above was
about the bundle's *named* packages; `onnxruntime-web`'s real bundled file is
`./node_modules/onnxruntime-web/dist/ort-web.min.js`, a pre-minified vendor file webpack rolled in
whole -- packages nested *inside* that file were already flattened before webpack's own
module-boundary comments could ever name them, so the pass above couldn't have found them. Checked
directly: `onnxruntime-web`'s real, published `package.json` (confirmed via its GitHub repo) lists
`flatbuffers`, `long`, `protobufjs`, `guid-typescript`, and `platform` as dependencies. Grepping the
actual bundled code for each package's own distinctive signatures (not just its name, which
minification can rename away) found real, functional evidence of two:
`r.Long=r.global.dcodeIO&&r.global.dcodeIO.Long||r.global.Long||r.inquire("long")` is
`protobufjs`'s own well-known Long-implementation compatibility shim (`dcodeIO` is that library's
author's npm/GitHub handle), and `r.flatbuffers.ByteBuffer`/`InferenceSession.getRootAsInferenceSession`
match `flatbuffers`'s generated-code API exactly, used here to parse the ORT model format. No
license/copyright banner text survived minification for either (unlike `onnxruntime-common`'s own
module, which kept its comment), so both are attributed here from each project's own real upstream
`LICENSE`, verified directly, not read out of the bundle:

### `protobufjs` -- BSD-3-Clause

Copyright (c) 2016, Daniel Wirtz. All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted
provided that the following conditions are met: (1) Redistributions of source code must retain the
above copyright notice, this list of conditions and the following disclaimer. (2) Redistributions
in binary form must reproduce the above copyright notice, this list of conditions and the following
disclaimer in the documentation and/or other materials provided with the distribution. (3) Neither
the name of its author, nor the names of its contributors may be used to endorse or promote
products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

`protobufjs`'s own `long` dependency (surfaced by the same code path above) is Daniel Wirtz's
separate `long.js` package, also BSD-3-Clause under the same copyright -- not reproduced as a
second entry since it is the exact same author, license, and terms.

### `flatbuffers` -- Apache-2.0

Copyright 2014 Google Inc.

Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except
in compliance with the License. You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software distributed under the License
is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
implied. See the License for the specific language governing permissions and limitations under the
License.

**Remaining gap, real but now much narrower.** `guid-typescript` and `platform` are both declared
`onnxruntime-web` dependencies but produced zero string evidence (by name or by known API
signature) anywhere in this bundle -- plausibly because they're used only by `onnxruntime-web`'s
Node-specific code paths, which this browser build tree-shakes out (`onnxruntime-node` itself is
confirmed elsewhere in this file to be an externalized stub, not real bundled code, for the same
reason). Not attributed here since no evidence of inclusion was found, but not confirmed absent
either -- a full rebuild-from-source with `license-checker`/`LicenseWebpackPlugin` remains the only
way to be fully certain nothing else is nested in.

---

## Provenance note: D3.js / d3-geo-projection / topojson-client

All three D3-family files above (but not Leaflet.js) carry a shared `/** @odoo-module **/` banner
line as their first line, which is Odoo's own asset-pipeline marker for a file importable as an ES
module in Odoo's frontend build -- `fetch_assets.py`'s plain download-and-hash logic for Leaflet
and Transformers.js does not add this banner, so these three came from some other, still-
undocumented vendoring process that does. One plausible source was checked and ruled out tonight:
Odoo core itself ships `topojson`-format *data* files (continent/country boundaries) under
`odoo/addons/spreadsheet/static/topojson/`, for its Spreadsheet dashboard geo-chart feature, and
that feature's chart library (`odoo/addons/spreadsheet/static/lib/chartjs-chart-geo/
chartjs-chart-geo.js`) was checked directly for a bundled copy of D3/topojson-client source --
it contains at most one incidental substring match, not a real embedded copy, so this is not where
these three files came from. **Resolved 2026-08-26**: the precise byte-level provenance is now
documented in `docs/proposals/VENDORED_ASSET_LICENSE_ATTRIBUTION.md`'s "Resolved:
D3.js/topojson-client provenance" section -- in short, all three are the stated versions' real
unpkg `dist/` builds with the banner prepended; `d3-geo-projection.v4.min.js` additionally has its
UMD wrapper's `require("d3-geo")`/`require("d3-array")` calls replaced with nonexistent
`importModule(...)` calls (a targeted fix for a real "d3-array module dependency" test failure a
naive fresh-fetch replacement reproduces, per that section). `fetch_assets.py` now has a
documented, hash-pinned reproduction of this transform, though it's deliberately not wired into
automatic execution -- see that function's own docstring.
