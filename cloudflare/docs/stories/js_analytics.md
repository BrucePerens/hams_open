# `cloudflare/static/src/components/analytics/analytics.js`

The Cloudflare analytics page shows requests, bandwidth and threats. Until the real figures are fetched it shows fixed sample ones. Tests: `static/tests/analytics.test.js`.

### `CloudflareAnalyticsDashboard.setup()` [@ANCHOR: cloudflare:js_analytics_setup]

The OWL `setup()` hook of the `CloudflareAnalyticsDashboard` component, registered as the client action `cloudflare_analytics_dashboard` with the template `cloudflare.AnalyticsDashboard`. It takes no arguments and returns nothing. It creates the reactive state `{requests: 0, bandwidth: "0 GB", threats: 0, loading: true}`, and registers an `onWillStart` callback that waits 500 milliseconds (a stand-in for a real fetch) and then sets the sample figures `requests = 154200`, `bandwidth = "45.2 GB"`, `threats = 1205` and `loading = false`, before the page is shown. The data is fixed sample data, not read from Cloudflare, and no request is made; there are empty mount and unmount hooks. Replacing the wait with a real API call is a to-do: the state shape is the contract the template reads. Test: "setup starts in the loading state and, once its half-second wait is over, shows the figures" in `static/tests/analytics.test.js`, which advances time by 500 ms and checks the three values and the rendered text.
