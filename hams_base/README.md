# Hams Base

The `hams_base` module provides foundational security, compliance, and email transport overrides for the Hams Open platform.

## Developer & AI Reference

### Email & DMARC Handling
* **`hams_base.dmarc.report` & `hams_base.dmarc.record`**:
  * Intercepts `not-read@` catch-all routes.
  * Parses incoming DMARC RUA (Aggregate) XML reports to analyze email alignment and SPF/DKIM validation failures.
* **`mail.thread` Overrides**:
  * Overrides the message routing logic to drop vacation replies and auto-responders to prevent mail loops.
  * Handles manual unsubscribe requests efficiently.
* **`res.partner` Overrides**:
  * `message_receive_bounce()`: Intercepts bounce messages and triggers internal alerts. It automatically notifies club officers or administrators when critical emails bounce.

### Security Alerts & Configuration
* **`res.users` Overrides**:
  * Implements security alerts that are dispatched directly to users upon sensitive profile modifications, such as email or login password changes.
* **`res.config.settings`**:
  * Manages compliance settings and exposes recommended SPF and DMARC DNS configurations for the instance domain.
