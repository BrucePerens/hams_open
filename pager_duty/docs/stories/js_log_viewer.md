# `pager_duty/static/src/components/log_viewer/log_viewer.js`

The log search page lets an operator pick a server log file and search it with a regular expression. Tests: `static/tests/log_viewer.test.js`.


The component is the OWL class `LogViewer`, registered as the client action `pager_duty.log_viewer` with the template `pager_duty.LogViewerTemplate`. It talks to the server with `fetch` against the JSON endpoints under `/api/v1/pager/logs/` (the controller documents access and limits in `docs/stories/log_search_board_and_service_accounts.md`), not through `orm`, so the server's own admin check decides what the operator may see.

## State

| Field | Type | Meaning |
|---|---|---|
| `files` | `list[str]` | The log file paths the server allows searching. |
| `selectedFile` | `str` | The file chosen in the list; the first file after loading. |
| `regexQuery` | `str` | The regular expression typed by the operator. |
| `results` | `list` | Matching lines of the last finished search. |
| `loading` | `bool` | True while a search job is running. |
| `error` | `str` or `null` | The message shown to the operator, or `null`. |

## `setup()` [@ANCHOR: pager_duty:js_log_viewer_setup]

Called once by OWL. No arguments, no return value. It gets the `orm` service (kept for the OWL service convention), creates `this.state` with empty values, `loading` false and `error` null, and registers an `onMounted` hook that awaits `fetchFiles()`. Nothing is read from the server until the component is mounted. If `fetchFiles` fails it handles its own error (it never rejects), so mounting always completes. No timers or subscriptions are created, so there is nothing to clean up on unmount. Test: `static/tests/log_viewer.test.js` ("setup loads the list of log files when mounted").

## `fetchFiles()` [@ANCHOR: pager_duty:js_log_viewer_fetch_files]

Asynchronous, no arguments, returns a promise that resolves to nothing. It sends `POST /api/v1/pager/logs/files` with the JSON-RPC body `{"params": {}}`, reads `result.files` (an empty list when missing) into `state.files` and, when the list is not empty, selects the first file in `state.selectedFile`. Any failure (network error, a reply that is not JSON) is caught and sets `state.error` to "Failed to load configuration."; the list stays empty. It does not retry. The server returns only files an operator may search, so the page does not filter. Test: "fetchFiles lists the files and picks the first, and says so when the list cannot be loaded".

## `onKeyup(ev)` [@ANCHOR: pager_duty:js_log_viewer_on_keyup]

The key handler of the pattern box. Argument: `ev`, a keyboard event; only `ev.key` is read. When it is `"Enter"` it starts `executeSearch()` without waiting for it (`void`), so the handler returns at once; any other key does nothing. Returns nothing. Errors from the search are handled inside `executeSearch`. This lets the operator search from the keyboard without pressing the button. Test: "onKeyup starts the search on Enter only".

## `executeSearch()` [@ANCHOR: pager_duty:js_log_viewer_execute_search]

Asynchronous; no arguments; returns a promise resolving to nothing, and never rejects. If no file is selected or the pattern is empty it sets `state.error` to "Please select a file and enter a regex pattern." and stops. Otherwise it sets `loading` true, clears `error` and `results`, and sends `POST /api/v1/pager/logs/search` with `{params: {file_path, regex_query}}`. A reply with `error` shows that text; a reply with a `job_id` starts polling `POST /api/v1/pager/logs/search_poll` once a second, up to 30 times. A poll that returns `error` shows it; one with `status: "done"` stores `matches` in `results`, or sets "0 matches found." when there are none; after 30 polls it shows "Search timed out.". A reply with neither shows "Invalid API response."; any thrown error shows "IPC Request Failed.". `loading` is reset in a `finally` block. The regular expression is run on the server by the log analyzer, not in the browser, so a bad or costly pattern cannot hang the page. Tests: "executeSearch asks for a file and a pattern first" and "executeSearch starts a job, polls until it is done, and shows the matches".
