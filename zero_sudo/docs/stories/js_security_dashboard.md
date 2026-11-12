# `zero_sudo/static/src/components/security_dashboard/security_dashboard.js`

The security dashboard is an Owl client action (`zero_sudo.security_dashboard`, template `zero_sudo.SecurityDashboard`) that shows
the newest security log entries (failed logins, blocked service accounts and the like) on a wall display. It reads the log through
the ORM service, so Odoo's access rules for `zero_sudo.security.log` decide who sees anything; it writes nothing. Tests:
`static/tests/security_dashboard.test.js`.

### `setup()` `[@ANCHOR: zero_sudo:js_security_dashboard_setup]`

No arguments; Owl calls it when the component is created. Gets the `orm` and `action` services and creates the state
`{logs: [], isPaused: false}` and a ref named `dashboardRoot`. Registers `loadLogs()` to finish before the first display
(`onWillStart`), so the page never shows an empty table that then jumps. Starts a 20 second timer (cleared when the component is
destroyed) that moves the root element by a random whole offset of -3 to +2 pixels in each direction, unless the display is paused,
so an OLED panel does not burn in on a fixed image. The effect's dependency argument is a function returning `[]` because Owl's
`useEffect` throws on an array. The list is not refreshed afterwards; the wall display reloads the action to update it.

### `loadLogs()` `[@ANCHOR: zero_sudo:js_security_dashboard_load_logs]`

No arguments. Returns a promise that resolves after `state.logs` is replaced with the result of a `searchRead` on
`zero_sudo.security.log` for the fields `id`, `create_date`, `user_id`, `login`, `ip_address` and `reason`, newest first, at most
100 rows. Rejects with the RPC error (for example an access error for a user who may not read the log), which Odoo shows in its
usual error dialog. The `user_agent` field is not requested. Read-only.

### `togglePause()` `[@ANCHOR: zero_sudo:js_security_dashboard_toggle_pause]`

No arguments, returns nothing. Flips `state.isPaused`, which the timer in `setup()` reads: paused stops the pixel shift (an
operator who is reading or photographing the screen pauses it); resumed shifts again at the next tick. It does not touch the
data or the log. Runs in the browser only.
