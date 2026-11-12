# `pager_duty/static/src/components/log_viewer/log_viewer.js`

The log search page lets an operator pick a server log file and search it with a regular expression. Tests: `static/tests/log_viewer.test.js`.

- `setup()` loads the list of log files when the page is mounted. [@ANCHOR: pager_duty:js_log_viewer_setup]
- `fetchFiles()` fills the file list and selects the first file, or says the configuration could not be loaded. [@ANCHOR: pager_duty:js_log_viewer_fetch_files]
- `onKeyup()` starts the search when Enter is pressed in the pattern box. [@ANCHOR: pager_duty:js_log_viewer_on_keyup]
- `executeSearch()` asks for a file and a pattern, starts a search job, polls it once a second for up to thirty seconds, and shows the matches or why there are none: no matches, a server error, an invalid reply, a timeout or a failed request. [@ANCHOR: pager_duty:js_log_viewer_execute_search]
