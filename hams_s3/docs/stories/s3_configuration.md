# Story: Configuring Amazon S3 Storage

## Context
Self-hosted instances often need to offload media/attachment storage to
Amazon S3 rather than the local filesystem, once the OCA `storage_backend`
family of modules is installed (see the module README for the setup
script). `hams_s3` adds the administrative UI for that, without hard-depending
on `storage_backend` itself so the module still installs cleanly before an
admin has run the setup script.

## The Solution
1. **Settings**: Administrators enable "Use Amazon S3 Storage" and enter the
   host, access key, secret key, bucket, and region from the "Cloud Storage"
   block Odoo's General Settings page gains once this module is installed
   ([@ANCHOR: COMM_hams_s3_config]). Saving writes (or updates) the matching
   `storage.backend` record via the module's own service account, rather
   than a hand-rolled `.sudo()` call.
2. **Rendering**: The settings block is injected into the standard
   `base_setup` settings form via an xpath extension whose compiled arch is
   proven directly (`get_view`) rather than through a browser tour, since a
   full "installed + configured" round-trip needs the OCA `storage_backend`
   addon actually present ([@ANCHOR: COMM_hams_s3_settings_render]).

## Impact
Administrators configure S3-backed attachment storage entirely from the
Odoo UI once the OCA dependency is installed, without ever needing direct
database or filesystem access to Odoo's storage configuration.
