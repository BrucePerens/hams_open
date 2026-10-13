# Vendored browser libraries added for QA-015 (`external/static/src/node_modules/`)

Third-party code the `external` module serves to the browser from this site, not from a CDN (QA-015, `docs/compliance/THIRD_PARTY_BROWSER_LOADS.md`
in hams_com). The older libraries (Leaflet, D3, topojson-client, noble, SIP.js, ft8js, Transformers.js) are described in `README.md`; this file
records the two added later, with their hashes. The pages are in hams_com's `ham_logbook` (QSL Collection, Awards Dashboard, Award Designer). Each file is byte-for-byte what the project published to npm; nothing is edited.
`tests/test_vendored_files.py` checks every sha256 below against the file on disk.

Fetched 2026-10-13 from the npm registry tarball (`https://registry.npmjs.org/<name>/-/<name>-<version>.tgz`), unpacked with `tar -x` only
(no install, no lifecycle script ran). The tarball's SHA-512 was compared with the registry's `dist.integrity`; the file was also
fetched from `https://unpkg.com/<name>@<version>/dist/...` and is byte-identical.

| Library | Version | File (under `static/src/node_modules/`) | Upstream path in the tarball | sha256 of the file | Licence |
|---------|---------|-----------|------------------------------|--------------------|---------|
| jsPDF | 4.2.1 | `jspdf/jspdf.umd.min.js` | `package/dist/jspdf.umd.min.js` | `e6551fcdc32f09d6853b2c5126d18d01d9447e0da618a41a11ebeee0f6c20d54` | MIT |
| fabric.js | 7.4.0 | `fabric/fabric.min.js` | `package/dist/index.min.js` (renamed, content unchanged) | `d4e908b3b3654db92c08ff0f31aa6a72ba1ddeeeb43a93bc41c833e51fe37f58` | MIT |

Tarball integrity (npm `dist.integrity`, matched): jspdf-4.2.1
`sha512-YyAXyvnmjTbR4bHQRLzex3CuINCDlQnBqoSYyjJwTP2x9jDLuKDzy7aKUl0hgx3uhcl7xzg32agn5vlie6HIlQ==`; fabric-7.4.0
`sha512-NalYDc3eifTl1C33zryQwpH6+XA/2ClxQrH9vkASkZw3tbkRmorpikhYMmxhUTmi7O3e9ODz0vOT8qfaCh9IVA==`.

Licence texts (sha256):

| File | sha256 |
|------|--------|
| `jspdf/LICENSE` | `dc388ec35ff463288cdde3588a36fd4ed12a45deff053243b37309acc9ef583b` |
| `jspdf/LICENSE.bundled-dependencies` | `957cba57becf54291edf76bbe988aae95a8be6fcc8ff838930859a83f214a887` |
| `fabric/LICENSE` | `eda412692b7398293a049ecf913319da26eb8f7fe27f10709821dd187b517e0b` |

## Why these versions, not the ones the CDN tags named

The pages loaded jsPDF 2.5.1 and fabric.js 5.3.0 (the 2.5.1 and 5.3.0 files match the pins in the Pacificon demo kit: sha256
`98ccf17a...3875` and `48f8f091...2ca`, and the tarball integrity above matched npm; the old tags carried no `integrity=`
attribute, so the page never verified them). Both are in the GitHub Advisory Database as vulnerable, with no fix on those lines:

* jsPDF: every advisory through 4.2.0 (PDF object injection, AcroForm JavaScript injection, DoS through GIF/BMP dimensions, ReDoS,
  path traversal in the Node build, ...); fixed in 4.2.1. The calls this site makes (`new jspdf.jsPDF`, `addImage`, `save`) are the same in 4.x.
* fabric.js: stored XSS through SVG export (CVE-2026-27013, fixed 7.2.0) and `Gradient` colour stops (CVE-2026-44311, fixed 7.4.0); the 5.x
  line (last 5.5.2) has no fix. `ham_logbook/static/src/js/award_designer.js` (hams_com) was ported to the 7.x API (promise-returning `Image.fromURL`,
  `backgroundImage` assignment, explicit `originX/originY: 'left'/'top'` because 7.x centres objects by default).

## Notices

* **jsPDF** `jspdf.umd.min.js` keeps the project's own `@license` header and the notices of the code it includes (rgbcolor, MD5 by Joseph
  Myers, FPDF, the PDF-encryption code, omggif, the Adobe JPEG encoder, Dominik Homberger's PNG code, Bidi, and others) as comments in the
  file. The project's licence text is `LICENSE`. The UMD build also bundles `fflate`, `fast-png` and `iobuffer` (all MIT; their licence
  texts are in `LICENSE.bundled-dependencies`, taken from the npm releases fflate 0.8.2, fast-png 6.4.0, iobuffer 5.4.0, the current ones
  inside the ranges jsPDF declares).
* jsPDF contains the string `https://cdnjs.cloudflare.com/ajax/libs/pdfobject/...`. It is used only by `doc.output("pdfobjectnewwindow")` and
  `"pdfjsnewwindow"`, which nothing here calls, and the policy would block it anyway.
* **fabric.js** is MIT (`LICENSE`); the minified file carries no further notices.
* Both files end with a `//# sourceMappingURL=` comment for a `.map` file that is deliberately not vendored. A browser asks for a source
  map only while its developer tools are open, and the request is to this site.

## Updating

Fetch the new tarball into an empty directory, compare `dist.integrity`, `tar -x` it there (never run its scripts), replace the file and the licence
files in `static/src/node_modules/<name>/`, update this table and `THIRD_PARTY_LICENSES.md`, and run `-u external` tests. Pages that load
the file are `<script src>` tags in hams_com's `ham_logbook/views/`.
