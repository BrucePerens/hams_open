# `zero_sudo/static/src/components/security_dashboard/security_dashboard.js`

The security dashboard shows the newest security log entries (failed logins and the like) on a wall display. Tests: `static/tests/security_dashboard.test.js`.

- `setup()` loads the log before the dashboard is shown, and every twenty seconds shifts the display by a few pixels, unless paused, so an OLED screen does not burn in. [@ANCHOR: zero_sudo:js_security_dashboard_setup]
- `loadLogs()` reads the newest hundred security log entries. [@ANCHOR: zero_sudo:js_security_dashboard_load_logs]
- `togglePause()` pauses or resumes the display shift. [@ANCHOR: zero_sudo:js_security_dashboard_toggle_pause]
