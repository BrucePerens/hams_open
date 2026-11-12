# `caching/static/src/js/toast.js`

The "Update Available" toast tells a visitor that a new version of the site has loaded in the background. It is the Owl component `SWToast`, registered in the `main_components` registry as `caching.SWToast`, so it is mounted on every page that renders the website's main components. It has one reactive field, `state.show` (a boolean from `useState`, initially `false`; `true` means the toast is drawn). It renders a Bootstrap toast fixed to the bottom right (`z-index: 9999`) with a close button wired to `close` and a "Reload" button wired to `reload`. The service worker side that sends the message is `/sw.js` (served by `controllers/main.py`). Tests: `static/tests/toast.test.js`. None of this touches the server, the database or the transmitter, and it handles no secrets.

### `setup()` [@ANCHOR: caching:js_toast_setup]

Owl's lifecycle hook, called once when the component is created. It sets `this.state = useState({ show: false })`, so the toast starts hidden. If the browser has `navigator.serviceWorker` it adds a `message` event listener to it; a browser without service workers gets no listener and never shows the toast. The listener looks at `event.data.type`: only the exact value `NEW_VERSION_INSTALLED` (posted by the service worker when a new cache version has installed) does anything, and it calls `_showOnceModalClear()` (anchor `caching:toast_deferred_while_modal_open`), which shows the toast at once or, if a Bootstrap modal such as the cookie-consent bar is open, after that modal is hidden. Every other message, and messages with no data, are ignored.

| Argument | Type | Meaning |
|---|---|---|
| none | | Owl passes props, but this component has none. |

Returns nothing; it cannot throw in normal use. Side effect: one permanent listener on the page's service worker container, never removed (the component lives as long as the page). Synchronous; the listener fires asynchronously on the page's event loop. Test: no direct test of `setup` (to-do); the deferral it calls is tested in `toast.test.js`.

### `close()` [@ANCHOR: caching:js_toast_close]

Click handler of the toast's close button. It sets `state.show = false`, so the next render drops the toast. Arguments: none (the click event is not read). Returns nothing, cannot fail, idempotent, no network. It does not dismiss the pending update: the new version still takes effect at the next page load. Test: `toast.test.js`, "close() hides the toast by setting state.show to false".

### `reload()` [@ANCHOR: caching:js_toast_reload]

Click handler of the toast's "Reload" button. It calls `document.location.reload()`, a normal browser reload of the current URL, so the page is fetched again and runs with the newly installed service worker cache. Arguments: none. Returns nothing; the page is torn down by the navigation, so unsaved form input on the page is lost (the visitor chose the button). It does not change `state`. Test: none by design (the file's header comment says it only calls the browser's reload and has no branching to verify).
