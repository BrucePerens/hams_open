# DMARC Aggregate Report Review

Receiving mail servers that support DMARC send periodic aggregate reports (RUA) describing whether
mail claiming to be from hams.com's domain passed SPF/DKIM alignment -- the standard way a domain
owner detects spoofing or misconfigured legitimate senders.

DMARC, RUA, SPF and DKIM are expanded in the Email & DMARC Handling section of [the module README](../../README.md).

## Epic: Reviewing DMARC Reports

* **Story:** As an administrator, I want to browse received DMARC aggregate reports in a list, so I
  can spot domains sending on our behalf or scan for alignment failures at a glance.
    * **BDD (behaviour-driven development) Criteria:**
        * *Given* one or more `hams_base.dmarc.report` records
        * *When* I open the report list
        * *Then* I see the reporting organization, domain, and evaluated policy for each report,
          with expired date ranges visually muted.
          *(Reference: the `view_dmarc_report_tree` entry below)*
* **Story:** As an administrator, I want to open a single report and see every underlying source-IP
  record it contains, so I can identify exactly which sending IP passed or failed alignment.
    * **BDD Criteria:**
        * *Given* a `hams_base.dmarc.report` record
        * *When* I open its form view
        * *Then* I see the report's own metadata (org, contact email, domain, evaluated policy) and
          a list of every `hams_base.dmarc.record` it contains, each showing source IP,
          disposition, and DKIM/SPF alignment (visually flagged pass/fail).
          *(Reference: the `view_dmarc_report_form` entry below)*
        * The same per-record list, shown standalone (outside a specific report's form), reads the
          same way.
          *(Reference: the `view_dmarc_record_tree` entry below)*

## Epic: Configuring the Compliance Email Footer

* **Story:** As an administrator, I want to set the organization name shown in every outbound
  notification email's compliance footer from the standard Settings screen, so I don't need direct
  database access to change it.
    * **BDD Criteria:**
        * *Given* the Settings app's General Settings page
        * *When* I open the "Hams Base" section under "Email Compliance & Reputation"
        * *Then* I can set `compliance_org_name`, the same value the notification-email footer
          reads (see `mail_notification_compliance_footer.md`).
          *(Reference: the `res_config_settings_view_form` entry below)*

## Entries

All views are in `hams_base/views/`. The models are `hams_base.dmarc.report` and `hams_base.dmarc.record`
(`hams_base/models/dmarc_report.py`); reports are created from the aggregate-report e-mails the fetchmail alias
receives, so these views are read-only windows onto parsed data.

### `hams_base.view_dmarc_report_tree` [@ANCHOR: view_dmarc_report_tree]

The list view of `hams_base.dmarc.report` (`views/dmarc_report_views.xml`, record id `view_dmarc_report_tree`, also
tagged `hams_base:view_dmarc_report_tree` on the XML record). Columns, in order: `date_range_begin` (Begin Date),
`org_name` (Organization), `domain`, `p` (the evaluated policy: none, quarantine or reject). The row attribute
`decoration-muted="date_range_end < current_date"` greys out a report whose date range ended before today, which is every
report after its day, so the muting separates current from past reports at a glance. No buttons, no editing; the list is
the first view of the "DMARC Reports" menu (`menu_dmarc_reports` under Settings, Technical menu, action
`action_dmarc_report`, `view_mode` list,form). Access follows the model's access rules. Test:
`tests/test_view_rendering_audit_cluster.py` (renders the view; fails on a missing field).

### `hams_base.view_dmarc_report_form` [@ANCHOR: view_dmarc_report_form]

The form view of one report. Everything is `readonly="1"`: reports come from outside, nobody edits them. The title is
`org_name`; the groups show `report_id`, `email` (email widget), `domain`, `date_range_begin`, `date_range_end`; a
"Policy Evaluated" section shows `p`, `sp`, `adkim`, `aspf` and `pct`; a "Records" tab holds the One2many `record_ids`
as an inline list of `source_ip`, `count`, `disposition`, `dkim_alignment` and `spf_alignment`. The inline list has no
pass/fail colouring (only the standalone record list has it; see the next entry), which differs from the BDD text
above: to-do: add the `decoration-*` attributes to the inline list or correct the story. The form also has the chatter
(followers and messages). Test: `tests/test_view_rendering_audit_cluster.py`.

### `hams_base.view_dmarc_record_tree` [@ANCHOR: view_dmarc_record_tree]

The standalone list view of `hams_base.dmarc.record` (id `view_dmarc_record_tree`, also tagged
`hams_base:view_dmarc_record_tree`). Columns: `source_ip`, `count` (messages seen from that IP), `disposition` (what the
receiver did: none, quarantine or reject), `dkim_alignment` and `spf_alignment`; each alignment cell is green when its
value is `pass` and red when it is `fail` (`decoration-success` / `decoration-danger`). Use it to find which sending
address failed alignment. Read-only; one row per source IP and policy result in the report. Test:
`tests/test_view_rendering_audit_cluster.py`.

### `res_config_settings_view_form` [@ANCHOR: res_config_settings_view_form]

The inherited Settings form (`views/res_config_settings_views.xml`, record `res_config_settings_view_form`, `inherit_id`
`base.res_config_settings_view_form`; the xpath is tagged `hams_base:res_config_settings_xpath`). It adds an app section
"Hams Base" with two blocks. "Email Compliance & Reputation" has `compliance_org_name` (the organization name in the
notification footer) and `compliance_mailing_address` (a text widget, the postal address in the footer). "DNS Reputation
Records" shows the read-only computed `dns_spf_record`, a toggle `enable_dmarc_instructions` that reveals the read-only
`dns_dmarc_record` and a note that the DKIM record must be copied from the mail provider. The values are stored as system
parameters `hams_base.compliance_org_name` and so on, and the footer reads them. Test:
`tests/test_view_rendering_audit_cluster.py::test_res_config_settings_view_form_renders`.
