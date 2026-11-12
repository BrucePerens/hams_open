# Data structures

### `UserWebsitesSEOController` [@ANCHOR: user_websites_seo:user_websites_seo_main_user_websites_seocontroller]

**Purpose and location.** The class has no docstring; from its definition it provides `user_blog_index`. Defined in `user_websites_seo/controllers/main.py:8` as a class; bases: `UserWebsitesController`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `user_blog_index`.

**See also:** test `test_controller_no_ssti_elevation (tests/test_seo_controller.py)`.

### `BlogBlogSEO` [@ANCHOR: user_websites_seo:user_websites_seo_blog_blog_blog_blog_seo]

**Purpose and location.** Extends the Odoo model ["blog.blog", "user.websites.seo.metadata.mixin"] with no new fields and the methods `_check_seo_write_permission`. Defined in `user_websites_seo/models/blog_blog.py:6` as an Odoo model; bases: `models.Model`. (model `blog.blog`; inherits ["blog.blog", "user.websites.seo.metadata.mixin"].)

**Fields.** none declared by this class.

**Class attributes.** `_name = "blog.blog"`; `_inherit = ["blog.blog", "user.websites.seo.metadata.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_seo_write_permission`.

**See also:** test `test_mixin_base_check_seo_write_permission_is_abstract (tests/test_seo_models.py)`.

### `BlogPostSEO` [@ANCHOR: user_websites_seo:user_websites_seo_blog_post_blog_post_seo]

**Purpose and location.** Extends the Odoo model ["blog.post", "user.websites.seo.metadata.mixin"] with no new fields and the methods `_check_seo_write_permission`. Defined in `user_websites_seo/models/blog_post.py:6` as an Odoo model; bases: `models.Model`. (model `blog.post`; inherits ["blog.post", "user.websites.seo.metadata.mixin"].)

**Fields.** none declared by this class.

**Class attributes.** `_name = "blog.post"`; `_inherit = ["blog.post", "user.websites.seo.metadata.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_seo_write_permission`.

**See also:** test `test_xpath_rendering_pages_posts (tests/test_seo_models.py)`.

### `ResUsersSEO` [@ANCHOR: user_websites_seo:user_websites_seo_res_users_res_users_seo]

**Purpose and location.** Extends the Odoo model ["res.users", "user.websites.seo.metadata.mixin"] with no new fields and the methods `SELF_WRITEABLE_FIELDS`, `_check_seo_write_permission`. Defined in `user_websites_seo/models/res_users.py:7` as an Odoo model; bases: `models.Model`. (model `res.users`; inherits ["res.users", "user.websites.seo.metadata.mixin"].)

**Fields.** none declared by this class.

**Class attributes.** `_name = "res.users"`; `_inherit = ["res.users", "user.websites.seo.metadata.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `SELF_WRITEABLE_FIELDS`, `_check_seo_write_permission`.

**See also:** test `test_controller_no_ssti_elevation (tests/test_seo_controller.py)`.

### `SEOMetadataMixin` [@ANCHOR: user_websites_seo:user_websites_seo_seo_metadata_mixin_seometadata_mixin]

**Purpose and location.** Model description: User Websites SEO Metadata Mixin. Defined in `user_websites_seo/models/seo_metadata_mixin.py:6` as an Odoo model; bases: `models.AbstractModel`. (model `user.websites.seo.metadata.mixin`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_name = "user.websites.seo.metadata.mixin"`; `_description = "User Websites SEO Metadata Mixin"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_seo_fields`, `_check_seo_write_permission`, `write`.

**See also:** test `test_check_access_rule_res_users_seo_self_write_survives_a_non_main_company (tests/test_seo_models.py)`.

### `UserWebsitesGroupSEO` [@ANCHOR: user_websites_seo:user_websites_seo_user_websites_group_user_websites_group_seo]

**Purpose and location.** Extends the Odoo model [
        "user.websites.group",
        "website.seo.metadata",
        "user.websites.seo.metadata with no new fields and the methods `_check_seo_write_permission`. Defined in `user_websites_seo/models/user_websites_group.py:7` as an Odoo model; bases: `models.Model`. (model `user.websites.group`; inherits [
        "user.websites.group",
        "website.seo.metadata",
        "user.websites.seo.metadata.)

**Fields.** none declared by this class.

**Class attributes.** `_name = "user.websites.group"`; `_inherit = [
        "user.websites.group",
        "website.seo.metadata",
        "user.websites.seo.metadata`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_seo_write_permission`.

**See also:** test `test_check_access_rule_res_users_seo_self_write_survives_a_non_main_company (tests/test_seo_models.py)`.

### `WebsitePageSEO` [@ANCHOR: user_websites_seo:user_websites_seo_website_page_website_page_seo]

**Purpose and location.** Extends the Odoo model ["website.page", "user.websites.seo.metadata.mixin"] with no new fields and the methods `_check_seo_write_permission`. Defined in `user_websites_seo/models/website_page.py:6` as an Odoo model; bases: `models.Model`. (model `website.page`; inherits ["website.page", "user.websites.seo.metadata.mixin"].)

**Fields.** none declared by this class.

**Class attributes.** `_name = "website.page"`; `_inherit = ["website.page", "user.websites.seo.metadata.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_seo_write_permission`.

**See also:** test `test_xpath_rendering_pages_posts (tests/test_seo_models.py)`.
