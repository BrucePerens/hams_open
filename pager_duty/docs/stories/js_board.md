# `pager_duty/static/src/components/board/board.js`

The NOC dashboard shows the active and recently resolved incidents, the check totals and who is on duty, and refreshes itself when the server says the board changed. Tests: `static/tests/board.test.js`.


The component is the OWL class `PagerBoard`, registered as the client action `pager_duty.board` with the template `pager_duty.PagerBoardTemplate`. It uses the `bus_service` and `orm` services. All members below belong to that class.

## State

`this.state` (OWL `useState`) is the data the template draws.

| Field | Type | Meaning |
|---|---|---|
| `active_incidents` | `list` | Open and acknowledged incidents from the server (name, source, severity, status, create_date, ack_name). |
| `resolved_incidents` | `list` | The most recently resolved incidents. |
| `on_duty` | `str` | Name of the person on call, `"None"` when nobody, `"Loading..."` before the first load. |
| `stats` | `object` | Check counts: `passing`, `failing`, `maintenance`, `never_run` (all 0 until loaded). |
| `transformStyle` | `str` | The CSS `transform` and `transition` text applied to the whole display for burn-in protection. |
| `isPaused` | `bool` | True while automatic refresh is paused by the operator. |

## `setup()` [@ANCHOR: pager_duty:js_board_setup]

Called once by OWL when the component is created. It takes no arguments and returns nothing. It gets the `bus_service` and `orm` services, creates `this.state` with the defaults above, and registers two hooks. On mount it adds the bus channel `pager_duty`, adds `onNotification` as the listener for the `notification` event, awaits the first `fetchData()` and then starts a 60-second `setInterval` (stored as `this.burnInTimer`) that calls `applyBurnInShift`. On unmount it clears that timer. The bus listener is not removed on unmount (the page is a full-screen action that lives as long as the tab). Errors: a failure of the first `fetchData` rejects the mount hook and appears in the browser console. No Odoo security is applied here; the server call checks the user's rights. Test: `static/tests/board.test.js` ("setup subscribes to the pager_duty channel, loads the board and shows the incidents on screen").

## `fetchData()` [@ANCHOR: pager_duty:js_board_fetch_data]

Asynchronous; no arguments; returns a promise that resolves to nothing. It reads the page address: when the query string has `website_id`, it parses it as an integer and passes it as the call context, which makes the board show that website only; otherwise the context is empty and the server uses the current website. It calls `orm.call("pager.incident", "get_board_data", [], {context})` and copies `active`, `resolved`, `on_duty` and `stats` of the answer into `this.state`, then calls `applyBurnInShift` so the display moves at every refresh. Errors: a network or server error rejects the promise; callers handle it (`onNotification` logs it, `setup` and `acknowledge` let it reach the console). A non-numeric `website_id` becomes `NaN` and is sent as is. It reads nothing from the user's input beyond the address. Test: "fetchData fills the state from the server's board data, passing the page's website id, and then shifts the display".

## `applyBurnInShift()` [@ANCHOR: pager_duty:js_board_apply_burn_in_shift]

Synchronous, no arguments, returns nothing. It picks two whole numbers at random, each from -15 to +14 (`Math.floor(Math.random() * 30) - 15`), and sets `state.transformStyle` to `transform: translate(<x>px, <y>px); transition: transform 20s linear;`. The 20-second linear transition makes the movement too slow to notice, while the whole display, text and colours, never sits on the same pixels for long, which protects an OLED wall display that shows the board all day. It uses `Math.random`, not a secure generator, because nothing depends on the value. Called after every `fetchData` and every 60 seconds by the timer started in `setup`. It cannot fail. Test: "applyBurnInShift moves the display by up to 15 pixels each way over twenty seconds".

## `acknowledge(incidentId)` [@ANCHOR: pager_duty:js_board_acknowledge]

Asynchronous. Argument: `incidentId` (`int`, the `pager.incident` id of the clicked row). It calls `orm.call("pager.incident", "action_acknowledge", [[incidentId]])`, which sets the incident to acknowledged on the server (stamping the time and the user who clicked), then awaits `fetchData()` so the board shows the new state without waiting for the bus message. Returns a promise resolving to nothing. The template shows the button only for open incidents. Errors: an access or network error rejects the promise and the board is not reloaded; the server enforces who may acknowledge. Acknowledging an incident that is already acknowledged is harmless on the server. Test: "acknowledge tells the server which incident and then reloads the board".

## `togglePause()` [@ANCHOR: pager_duty:js_board_toggle_pause]

Synchronous, no arguments, no return value. It flips `state.isPaused`. While it is true, `onNotification` ignores update messages, so an operator reading or photographing the board is not disturbed by it changing. It does not stop the burn-in movement and does not fetch on resume: the board shows the old data until the next update message or an acknowledgement. The state is local to the open page and is not saved. Test: "togglePause flips between paused and live updates".

## `onNotification({ detail: notifications })` [@ANCHOR: pager_duty:js_board_on_notification]

The handler of the bus `notification` event. Argument: an event whose `detail` is the list of notification objects that arrived (each with `type` and a payload). Returns nothing. If the board is paused it returns at once. Otherwise, for each notification whose `type` is `update_board` (sent by the server's incident `create` and `write` on the `pager_duty` channel), it starts `fetchData()` and logs a failed reload with `console.error("[PagerBoard] fetchData failed:", err)`, so one failed reload does not stop later ones. Other notification types are ignored. Several update messages in one batch start several reloads. Test: "onNotification reloads the board for an update_board message, ignores other messages and does nothing while paused".
