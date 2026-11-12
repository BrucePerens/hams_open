# `user_websites/static/src/js/violation_report.js`

The "Report a violation" modal on a user's page is opened by a button that names the address being reported. Tests: `static/tests/violation_report.test.js`.

- `ViolationReportModal.start()` listens for the report modal opening, when the page has one. [@ANCHOR: user_websites:js_violation_report_start]
- `ViolationReportModal._onModalShow()` puts the address from the button that opened the modal into the form and clears any description and email left from last time; it does nothing when the modal was opened without a button. [@ANCHOR: user_websites:js_violation_report_on_modal_show]
