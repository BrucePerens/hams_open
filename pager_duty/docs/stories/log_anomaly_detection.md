# Story: Finding the Needle in the Haystack

## Persona
**Dave**, a DevOps Engineer investigating a intermittent "502 Bad Gateway" error that only happens during peak traffic.

## Context
Standard HTTP pings show the system is up, but users are complaining about occasional timeouts.

## The Hunt
Dave uses the Pager Duty Log Analyzer to find the root cause.

1.  **Regex Discovery:** Dave creates a new Log Pattern in Odoo [@ANCHOR: test_log_analyzer_views] searching for "upstream timed out" in the Nginx logs.

2.  **API Streaming:** The Log Analyzer daemon, running as a privileged background process, begins tailing the logs and streaming matches to the Odoo backend [@ANCHOR: pd_log_api_i18n].

3.  **Correlation:** Dave sees a spike in these errors every time the "Synthetic Journey Spooler" [@ANCHOR: synthetic_i18n] runs a heavy check against the payment gateway.
4.  **Optimization:** Dave adjusts the jitter and timeout settings in the synthetic check configuration to reduce the load on the Nginx upstream during peak hours.

## Result
The intermittent 502s disappear, and Dave has successfully used the integrated log forensics to solve a complex performance bottleneck.
