# AREDN gateway forms

Both forms use the same `hams-node` software. Facts: AREDN 4.26.10.0 (OpenWrt 25.12) routes IPv4 with Babel;
mesh-to-WAN is off by default; RF links are unencrypted; packages come from the node's admin package store.

## Form 1: AREDN node runs hams-node

```
 AREDN mesh (RF, IPv4 10.x, Babel)
        |
 [AREDN node + hams-node package]--- Internet (WireGuard) --- hams.com route servers
   Jool SIIT: 10.a.b.c <-> <block>:<domain>::10.a.b.c
```

The package is uploaded in Package settings. It adds a Jool SIIT instance per domain and WireGuard peers on the
node's Internet (WAN) link. It does not change AREDN's Babel, dnsmasq or firewall zones, and WireGuard never
runs over an RF link (47 CFR 97.113(a)(4)).

## Form 2: hams-node router on the LAN of an AREDN node

```
 AREDN mesh --- [AREDN node] --- LAN --- [hams-node router] --- Internet (WireGuard) --- route servers
```

The AREDN node is untouched and needs no package. The router reaches the mesh through the node's LAN, so it
sees AREDN's IPv4 space as a stub network; use this where the node cannot take another package or its flash
is small. Mesh-to-WAN stays off. The router uses the same packages as any hams-node router.

## What each form needs from the registry

One mesh domain ID (and its /96, from the registry export) per mesh, which `hams-node-siit render` turns into
Jool instance files. Part 97: nothing encrypted is passed onto the mesh.
