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
