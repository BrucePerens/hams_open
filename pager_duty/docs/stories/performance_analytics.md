# Story: The Data-Driven Post-Mortem

## Persona
**Charlie**, the VP of Engineering who focuses on operational efficiency and team burnout.

## Context
It's the first Monday of the month. Charlie is reviewing the team's performance metrics to see if the recent infrastructure upgrades have reduced incident response times.

## The Analysis
Charlie opens the Pager Duty Dashboard [@ANCHOR: test_pager_board_url].

1.  **Health Summary:** He immediately sees the real-time status of all monitoring checks, ensuring no critical services are currently failing [@ANCHOR: pager_board_stats].

2.  **Overview:** He views the aggregated Mean Time To Acknowledge (MTTA) and Mean Time To Resolve (MTTR) metrics [@ANCHOR: pager_board_data].

3.  **Drill Down:** He notices an incident from last week that had an unusually high MTTA. He clicks into the record [@ANCHOR: action_acknowledge_incident].

4.  **Timeline:** By reviewing the chatter, he sees that the incident fired during a shift hand-off. He notices that the escalation cron [@ANCHOR: test_pager_escalation] correctly triggered after 15 minutes because the primary on-call engineer was transitioning between home and office.

5.  **Action Plan:** Charlie decides to implement a 5-minute overlap in the on-call calendar shifts [@ANCHOR: test_pager_notification] to ensure coverage during hand-offs.

## Conclusion
With the hard data provided by the `pager.incident` model, Charlie can make informed decisions to improve team morale and system reliability without relying on anecdotes.
