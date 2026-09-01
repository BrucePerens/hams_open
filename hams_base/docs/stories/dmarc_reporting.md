# DMARC Aggregate Report Review

Receiving mail servers that support DMARC send periodic aggregate reports (RUA) describing whether
mail claiming to be from hams.com's domain passed SPF/DKIM alignment -- the standard way a domain
owner detects spoofing or misconfigured legitimate senders.

## Epic: Reviewing DMARC Reports

* **Story:** As an administrator, I want to browse received DMARC aggregate reports in a list, so I
  can spot domains sending on our behalf or scan for alignment failures at a glance.
    * **BDD Criteria:**
        * *Given* one or more `hams_base.dmarc.report` records
        * *When* I open the report list
        * *Then* I see the reporting organization, domain, and evaluated policy for each report,
          with expired date ranges visually muted.
          *(Reference: [@ANCHOR: view_dmarc_report_tree])*
* **Story:** As an administrator, I want to open a single report and see every underlying source-IP
  record it contains, so I can identify exactly which sending IP passed or failed alignment.
    * **BDD Criteria:**
        * *Given* a `hams_base.dmarc.report` record
        * *When* I open its form view
        * *Then* I see the report's own metadata (org, contact email, domain, evaluated policy) and
          a list of every `hams_base.dmarc.record` it contains, each showing source IP,
          disposition, and DKIM/SPF alignment (visually flagged pass/fail).
          *(Reference: [@ANCHOR: view_dmarc_report_form])*
        * The same per-record list, shown standalone (outside a specific report's form), reads the
          same way.
          *(Reference: [@ANCHOR: view_dmarc_record_tree])*

## Epic: Configuring the Compliance Email Footer

* **Story:** As an administrator, I want to set the organization name shown in every outbound
  notification email's compliance footer from the standard Settings screen, so I don't need direct
  database access to change it.
    * **BDD Criteria:**
        * *Given* the Settings app's General Settings page
        * *When* I open the "Hams Base" section under "Email Compliance & Reputation"
        * *Then* I can set `compliance_org_name`, the same value the notification-email footer
          reads (see `mail_notification_compliance_footer.md`).
          *(Reference: [@ANCHOR: res_config_settings_view_form])*
