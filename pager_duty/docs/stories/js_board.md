# `pager_duty/static/src/components/board/board.js`

The NOC dashboard shows the active and recently resolved incidents, the check totals and who is on duty, and refreshes itself when the server says the board changed. Tests: `static/tests/board.test.js`.

- `setup()` subscribes to the pager_duty bus channel, loads the board when the page is mounted, starts a timer that shifts the display every minute, and stops it when the page goes away. [@ANCHOR: pager_duty:js_board_setup]
- `fetchData()` reads the board from the server (for one website when the address names one), fills the page and shifts the display. [@ANCHOR: pager_duty:js_board_fetch_data]
- `applyBurnInShift()` moves the whole display by up to 15 pixels in each direction, slowly, so no pixel of an OLED screen shows the same thing all day. [@ANCHOR: pager_duty:js_board_apply_burn_in_shift]
- `acknowledge()` marks one incident acknowledged and reloads the board. [@ANCHOR: pager_duty:js_board_acknowledge]
- `togglePause()` pauses or resumes the board's automatic refresh. [@ANCHOR: pager_duty:js_board_toggle_pause]
- `onNotification()` reloads the board for each update message from the server, unless it is paused, and logs a failed reload. [@ANCHOR: pager_duty:js_board_on_notification]
