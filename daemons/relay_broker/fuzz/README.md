# relay_broker fuzz targets

libFuzzer targets for the broker's untrusted input: `wire_request` and `wire_response` (`Request::decode` and
`Response::decode`; whatever decodes must re-encode to something that decodes to the same value, and
`frame_body_len` must not panic) and `config_parse` (`Config::parse`).

Build and run exactly as in `../../radiod_client/fuzz/README.md` (own workspace root; `cargo +nightly fuzz run <target>`,
or the stable `RUSTC_BOOTSTRAP` recipe there).
