# `cloudflare/static/src/components/analytics/analytics.js`

The Cloudflare analytics page shows requests, bandwidth and threats. Until the real figures are fetched it shows fixed sample ones. Tests: `static/tests/analytics.test.js`.

- `setup()` starts the dashboard in the loading state and fills it with the sample figures after a short wait, before the page is shown. [@ANCHOR: cloudflare:js_analytics_setup]
