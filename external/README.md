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
