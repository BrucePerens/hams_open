# `user_websites/static/src/js/owner_delete_dialog.js`

The owner dashboard confirms before deleting a page or post, in a dialog that the delete links open. Tests: `static/tests/owner_delete_dialog.test.js`.

- `OwnerDeleteDialog.start()` makes each delete link fill in the dialog's form address, kind and title, and open it instead of following the link, and makes Cancel close it; in a browser that cannot show a modal dialog it does nothing, so the links work as plain links. [@ANCHOR: user_websites:js_owner_delete_dialog_start]
