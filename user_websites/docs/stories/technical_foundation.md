# Story: Technical Foundation and Utilities

As a **Developer**, I want to use reliable utility functions and a robust security model so that the module remains maintainable and secure.

## Scenarios

### Slug Generation
- **When** a user's name or a group's name needs to be converted into a URL-friendly slug.
- **Then** the system uses a robust `slugify` utility ([@ANCHOR: utils_slugify]). Verified by `[@ANCHOR: test_utils_slugify]`.
- **And** it handles special characters and normalization consistently.

### Documentation Access
- **When** I need help using the module.
- **Then** I can navigate to the documentation route ([@ANCHOR: controller_user_websites_documentation]). Verified by `[@ANCHOR: test_documentation_route]`.
- **And** the system attempts to redirect me to the appropriate `knowledge.article` if available.

## Technical Notes
- The module relies on a specialized service account for most background and initialization tasks ([@ANCHOR: mixin_proxy_ownership_create]). Verified by `[@ANCHOR: test_mixin_ownership_validation]`.
- Frontend notifications for administrators are powered by a lightweight RPC endpoint ([@ANCHOR: api_pending_reports]). Verified by `[@ANCHOR: test_admin_violation_toast_rpc]`.

### Extensibility Dropzones
The module provides several dropzones for UI extension:
- **Home Header:** [@ANCHOR: dropzone_home_header]
- **Home Footer:** [@ANCHOR: dropzone_home_footer]
- **Global Navbar:** [@ANCHOR: dropzone_navbar]
- **Navbar Actions:** [@ANCHOR: dropzone_navbar_actions]
- **Portal Templates:** [@ANCHOR: dropzone_templates]
- **Snippets Sidebar:** [@ANCHOR: dropzone_snippets]
- **Website Layout:** [@ANCHOR: dropzone_layout]
- **User Settings:** [@ANCHOR: dropzone_users]
- **Blog Post Form:** [@ANCHOR: dropzone_blog_post]
- **Directory Card:** [@ANCHOR: dropzone_directory_card]
- **Global Settings:** [@ANCHOR: dropzone_settings]

### Performance and Reliability
- **Slug Invalidation:** Distributed caches are invalidated upon slug mutation ([@ANCHOR: slug_cache_invalidation_unlink], [@ANCHOR: group_slug_cache_invalidation_unlink]).
- **View Counter Sync:** High-throughput view counters are flushed to the database via cron ([@ANCHOR: ir_cron_flush_view_counters]).
- **Weekly Digest:** Automated subscriptions are handled by a robust cron process ([@ANCHOR: ir_cron_send_weekly_digest]).
- **Moderation Alerts:** Admins are notified of pending reports ([@ANCHOR: ir_cron_notify_pending_reports]).
- **Simulation:** End-to-end operational load is verified in the simulation environment ([@ANCHOR: simulation_environment]).
- **Sanitization:** User-provided HTML is sanitized to prevent XSS ([@ANCHOR: website_page_sanitize_arch]).
- **Toast Notifications:** Feedback is provided via native notifications ([@ANCHOR: toast_notifications_logic], [@ANCHOR: admin_toast_logic]).
- **Violation Reporting:** Form submission is handled securely ([@ANCHOR: violation_report_logic]).
