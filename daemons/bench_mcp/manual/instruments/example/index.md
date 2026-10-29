---
title: Example instrument (template)
summary: The skeleton instrument that new drivers are copied from. An imaginary SCPI signal source; no hardware exists.
see_also: [concepts/extending, procedures/add-an-instrument, instruments/example/overview]
instrument: example
verified: n/a (navigation)
---

# Example instrument (template)

`"driver": "example"` is not a real instrument. It is the working skeleton in `src/example/` that a contributor copies to add a driver, with its manual pages, tests and fixture. It is listed only on a bench whose config contains an `example` entry. If you are an agent adding an instrument, read [Extending bench_mcp](concepts/extending) first. These pages are the model for the pages you write.

<!-- children -->
