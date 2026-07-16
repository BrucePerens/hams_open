# Cloudflared Daemon

This directory contains the upstream `cloudflared` client source, responsible for proxying traffic between the Cloudflare network and local origins without requiring open firewall ports.

### Context within `hams_open`
While this directory contains the standard Cloudflare Tunnel source, it is packaged and managed here to ensure a controlled and reliable version is compiled and deployed within the `hams_open` environment for secure tunnel routing.
