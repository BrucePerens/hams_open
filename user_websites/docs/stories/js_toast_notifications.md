# `user_websites/static/src/js/toast_notifications.js`

Two page scripts tell people what happened: a visitor after a form, and a site administrator about reports waiting for review. Tests: `static/tests/toast_notifications.test.js`.

- `UrlToastNotification.start()` checks the address for a notification to show as the page opens. [@ANCHOR: user_websites:js_toast_notifications_url_start]
- `UrlToastNotification._checkUrlForNotifications()` turns a success parameter (report, appeal, subscription, erasure) or a refusal reason in the address into a notification, and then removes the parameters from the address so a reload does not repeat it. [@ANCHOR: user_websites:js_toast_notifications_check_url]
- `AdminViolationToast.start()` checks for pending violation reports as the page opens. [@ANCHOR: user_websites:js_toast_notifications_admin_start]
- `AdminViolationToast._checkPendingReports()` asks the server how many reports wait, shows a sticky warning with a Review button when the queue is new or has grown, remembers that it did, and ignores a failed request. [@ANCHOR: user_websites:js_toast_notifications_check_pending_reports]
- `readSeen()` reads what the browser remembers of the last moderation toast, and gives nothing when there is none or the storage is blocked. [@ANCHOR: user_websites:js_toast_notifications_read_seen]
- `writeSeen()` remembers the count, newest report and time of a shown toast, and does nothing when the storage is blocked. [@ANCHOR: user_websites:js_toast_notifications_write_seen]
- `shouldShowToast()` says whether to show the toast: when the queue is not empty and was never shown, was shown twelve hours ago or more, or has grown. [@ANCHOR: user_websites:js_toast_notifications_should_show_toast]
