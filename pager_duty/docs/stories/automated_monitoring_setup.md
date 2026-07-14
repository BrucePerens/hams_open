# Story: Scaling the Watchtower

## Persona
**Bob**, an Infrastructure Administrator tasked with onboarding a new microservice into the monitoring fleet.

## Context
The company has just deployed a new "Order Processing" service. Bob needs to ensure it's monitored for availability and that any crashes are reported immediately.

## The Workflow
The system was recently installed, and Bob noticed that the documentation was automatically available in the Knowledge module upon installation [@ANCHOR: doc_inject_pager_duty]. He used this documentation to understand the setup process.

Bob opens the Odoo backend and navigates to the Monitoring Checks view [@ANCHOR: test_pager_view].

1.  **Creation:** He creates a new `pager.check` record of type "HTTP". He enters the URL of the order service's health endpoint.
2.  **Configuration:** Bob sets the interval to 60 seconds and assigns the "Odoo XML-RPC Handshake" as the parent check to ensure he doesn't get flooded with order service alerts if the entire network goes down.
3.  **Synchronization:** Bob clicks the "JSON Configuration Tools" wizard [@ANCHOR: generalized_pager_config]. He reviews the generated JSON and clicks "Export to JSON". This pushes the new configuration to the daemon's persistent storage.

4.  **Verification:** The `generalized_monitor.py` daemon reloads the configuration on its next tick. Bob watches the NOC Board [@ANCHOR: pager_board_data] and sees the new "Order Processing" check appear in the green "Healthy" state.

## Success
Bob can now rest easy knowing that the new service is under the protection of the Pager Duty guardian, and he didn't have to touch a single server terminal to set it up.
