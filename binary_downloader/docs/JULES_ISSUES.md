# Jules VM Testing Issues - binary_downloader

## Provisioning Issues
- No issues encountered during provisioning.

## Standard Test Issues
- `TestBinaryDownloaderTour.test_binary_install_tour` skipped with error: `Failed to detect chrome devtools port after 10.0s.`
- Chrome headless failed to start during the UI tour test with multiple D-Bus connection errors: `Failed to connect to the bus: Failed to connect to socket /dev/null: Connection refused`.
