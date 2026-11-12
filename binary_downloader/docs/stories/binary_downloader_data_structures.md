# Data structures

### `BinaryManifest` [@ANCHOR: binary_downloader:binary_downloader_binary_manifest_binary_manifest]

**Purpose and location.** Model description: Binary Download Manifest. Defined in `binary_downloader/models/binary_manifest.py:15` as an Odoo model; bases: `models.Model`. (model `binary.manifest`; inherits ["binary_downloader.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Binary Name: Command name (e.g., kopia) (required=`True`) |
| `url` | `Char` | Download URL (required=`True`) |
| `checksum` | `Char` | SHA-256 Checksum (required=`True`) |
| `archive_type` | `Selection` | Archive Type (values `binary`, `tar.gz`, `zip`; required=`True`; default=`"binary"`) |
| `extract_member` | `Char` | Extract Member: Specific file to extract from the archive. |
| `company_id` | `Many2one` | Company (comodel `res.company`; required=`False`; default=`lambda self: self.env.company`; index=`True`) |
| `is_installed` | `Boolean` | Installed (compute=`"_compute_is_installed"`) |

**Class attributes.** `_inherit = ["binary_downloader.mixin"]`; `_name = "binary.manifest"`; `_description = "Binary Download Manifest"`; `_msg_name_uniq = """The binary name must be unique per company!"""`; `_name_uniq = models.Constraint("UNIQUE(name, company_id)", _msg_name_uniq)`; `_msg_name_not_empty = """The binary name cannot be empty."""`; `_name_not_empty = models.Constraint("CHECK(LENGTH(TRIM(name)) > 0)", _msg_name_not_empty)`; `_msg_url_not_empty = """The download URL cannot be empty."""`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_url_scheme`, `_check_name_no_slashes`, `_check_extract_member`, `_compute_is_installed`, `action_install`, `ensure_executable`, `unlink`.

**See also:** test `test_01_already_installed (tests/test_binary_manifest.py)`.

### `BinaryTenantLink` [@ANCHOR: binary_downloader:binary_downloader_binary_tenant_link_binary_tenant_link]

**Purpose and location.** Model description: Tenant to Binary Version Assignment. Defined in `binary_downloader/models/binary_tenant_link.py:12` as an Odoo model; bases: `models.Model`. (model `binary.tenant.link`; inherits ["mail.thread", "mail.activity.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `website_id` | `Many2one` | Tenant / Website (comodel `website`; required=`True`; index=`True`; ondelete=`"cascade"`) |
| `manifest_id` | `Many2one` | Software (comodel `binary.manifest`; required=`True`; index=`True`; ondelete=`"cascade"`) |
| `active_version_id` | `Many2one` | Active Version (comodel `binary.version`; required=`True`; index=`True`) |
| `symlink_path` | `Char` | Tenant Execution Path (compute=`"_compute_symlink_path"`) |

**Class attributes.** `_inherit = ["mail.thread", "mail.activity.mixin"]`; `_name = "binary.tenant.link"`; `_description = "Tenant to Binary Version Assignment"`; `_tenant_manifest_uniq = models.Constraint("unique(website_id, manifest_id)", "A tenant can only have one active version of a`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_symlink_path`, `apply_symlink`, `create`, `write`, `unlink`, `action_upgrade_to_latest`.

**See also:** test `test_05_2_tenant_link_views_render (tests/test_binary_manifest.py)`.

### `BinaryDownloaderMixin` [@ANCHOR: binary_downloader:binary_downloader_binary_utils_binary_downloader_mixin]

**Purpose and location.** Model description: Binary Downloader Mixin. Defined in `binary_downloader/models/binary_utils.py:94` as an Odoo model; bases: `models.AbstractModel`. (model `binary_downloader.mixin`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name |

**Class attributes.** `_name = "binary_downloader.mixin"`; `_description = "Binary Downloader Mixin"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_download_and_extract`, `_assert_host_is_ssrf_safe`, `_atomic_write_target`, `_get_target_filename`, `_count_binary_file_references`, `_unlink_binary_file`.

**See also:** test `test_01_already_installed (tests/test_binary_manifest.py)`.

### `BinaryVersion` [@ANCHOR: binary_downloader:binary_downloader_binary_version_binary_version]

**Purpose and location.** Model description: Specific Binary Version Release. Defined in `binary_downloader/models/binary_version.py:14` as an Odoo model; bases: `models.Model`. (model `binary.version`; inherits ["binary_downloader.mixin"]; default order "release_date desc, id desc".)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `manifest_id` | `Many2one` | Software Manifest (comodel `binary.manifest`; required=`True`; index=`True`; ondelete=`"cascade"`) |
| `version_number` | `Char` | Version (required=`True`) |
| `release_date` | `Date` | Upstream Release Date |
| `url` | `Char` | Download URL (required=`True`) |
| `checksum` | `Char` | SHA-256 Checksum (required=`True`) |
| `archive_type` | `Selection` | Archive Type (values `binary`, `tar.gz`, `zip`; required=`True`; default=`"binary"`) |
| `extract_member` | `Char` | Extract Member: Specific file to extract from the archive. |
| `is_downloaded` | `Boolean` | Downloaded to Central Pool (compute=`"_compute_is_downloaded"`) |

**Class attributes.** `_inherit = ["binary_downloader.mixin"]`; `_name = "binary.version"`; `_description = "Specific Binary Version Release"`; `_order = "release_date desc, id desc"`; `_version_uniq = models.Constraint("UNIQUE(manifest_id, version_number)", "This version number already exists for thi`; `_url_not_empty = models.Constraint("CHECK(LENGTH(TRIM(url)) > 0)", "The download URL cannot be empty.")`; `_chksum_not_empty = models.Constraint("CHECK(LENGTH(TRIM(checksum)) > 0)", "The checksum cannot be empty.")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_version_no_slashes`, `_check_url_scheme`, `_check_extract_member`, `_get_central_path`, `_compute_is_downloaded`, `action_download_to_pool`, `action_notify_tenants`, `unlink`.

**See also:** test `test_01_already_installed (tests/test_binary_manifest.py)`.
