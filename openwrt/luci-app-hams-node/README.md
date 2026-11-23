# luci-app-hams-node

LuCI page (Services > hams.com Net) for the `hams-node` package, OpenWrt 25.12, client-side JS view.

- Status (read-only, polled every 5 s through the rpcd exec plugin `hams-node`): enrolment state, delegated /60, `.ham` name
  (`uci hams_node.main.ham_name`), active WireGuard peers (`/tmp/hams-node-ondemand/peers`), Part 97 boundary counters
  `refused_encrypted` and `refused_other` (`nft list counter inet hams_boundary`), and the integrity-only indicator (boundary loaded).
- Enrol button: runs `hams-node-enroll enroll` in the background and shows its code; the device token and the WireGuard private key are
  only tested for existence, never read into any reply.
- Settings (uci `hams_node_ondemand.main`): enable, and the uplink type declaration (`internet` or `rf`; `rf` means no WireGuard).
- ACL `luci-app-hams-node`: read `ubus hams-node status` and the two uci configs; write `enroll_start` and the two uci configs.

Tests: `python3 -m unittest discover -s tests` (backend with uci/nft stand-ins, and the ACL and menu files).
