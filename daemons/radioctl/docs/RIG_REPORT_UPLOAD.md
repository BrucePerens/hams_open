# Rig report upload contract

`radioctl-selftest` (crates/radioctld/src/upload.rs, `build_payload`) uploads one report per tested radio at the
end of a run. This file is the contract the hams.com receiver is built to; it must match the code exactly (a test in
`upload.rs` builds a payload and checks these fields).

- Endpoint: `POST https://hams.com/api/rig-reports` (constant `DEFAULT_URL`; `--server URL` overrides, https or
  http to loopback only). TLS 1.2/1.3 via rustls with the webpki root list.
- Request: `Content-Type: application/json`, body at most 262144 bytes (256 KiB), `Content-Length` set,
  `Connection: close`, `User-Agent: radioctl-selftest/VERSION`. Any 2xx is success; the client reads nothing from the
  body. The client gives up after 10 seconds in total.
- One request per tested radio, each with its own `report_id`.
- Never sent: host name, user name, file paths, IP addresses (the server sees the connecting address and should not
  keep it beyond rate limiting), USB serial numbers.

```json
{
  "schema_version": 1,
  "report_id": "UUID v4, made per run per radio",
  "sent_at": "2026-10-31T12:34:56Z",
  "radioctl_version": "0.1.0",
  "description_id": "icom-ic-7300",
  "rig": {
    "manufacturer": "Icom",
    "model": "IC-7300",
    "firmware_or_version": "Firmware(1, 2)  or null"
  },
  "host": { "os": "linux", "arch": "x86_64" },
  "link": {
    "transport": "usb | serial | virtual",
    "baud": 19200,
    "usb_vid_pid": "10c4:ea60  (only when the port has USB ids)"
  },
  "checks": [
    {
      "name": "read frequency A",
      "ok": true,
      "summary": "Frequency(Hz(14074000))",
      "raw_hex": "0.012 tx fe fe 94 e0 03 fd\n0.020 rx ...  (only when bytes were exchanged; at most 2048 characters)"
    }
  ],
  "baud_probe": { "tried": [4800, 9600, 19200], "chosen": 19200 },
  "contact": { "name": "only if given", "email": "only if given" }
}
```

Field notes:

- `rig.manufacturer` and `rig.model` come from the description's display name split at its first space
  (`"Icom IC-7300"`); `manufacturer` is empty when the name has no space. `firmware_or_version` is the summary text
  of the `identify: firmware` check when it passed (the description claims the identify read), else `null`. There is
  no `hamlib_model_id` field: descriptions do not carry one.
- `checks[].ok` is true for a pass; a failed check and a check the description does not support ("not applicable")
  are both false, and `summary` says which. `name` is at most 80 characters, `summary` at most 300.
- `raw_hex` lines are `SECONDS tx|rx HEX BYTES`, the wire log of that check. If the whole document would exceed
  256 KiB, `raw_hex` is removed from the last checks backwards, then summaries are emptied, until it fits.
- `baud_probe` is present only for `--baud auto`; `chosen` is the last rate tried, the one that answered.
- `contact` is always present; it holds `name` and/or `email` only if the volunteer gave them. `name` is at most 120
  characters, `email` at most 254 and has the shape `x@y.z`.
- Strings are UTF-8 JSON. `sent_at` is UTC.
