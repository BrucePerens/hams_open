# `caching/static/src/js/toast.js`

The "Update Available" toast tells a visitor that a new version of the site has loaded in the background. Tests: `static/tests/toast.test.js`.

- `setup()` starts hidden and listens for the service worker's message that a new version is installed, then shows the toast (once no modal is open). [@ANCHOR: caching:js_toast_setup]
- `close()` hides the toast. [@ANCHOR: caching:js_toast_close]
- `reload()` reloads the page so it runs the new version. [@ANCHOR: caching:js_toast_reload]
