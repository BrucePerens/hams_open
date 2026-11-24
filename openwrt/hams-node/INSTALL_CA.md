# Installing the node's proxy CA

Your router made its own certificate authority. It can only vouch for `.ham` names, the whitelisted sites and the ham network's addresses; a
certificate it signs for any other site is rejected by your browser. Its private key never leaves the router. Download the certificate
(`ca.pem`, a public file) from the router's LuCI page "hams.com Net", then install it:

- **Windows** (Chrome, Edge): double-click `ca.pem`, Install Certificate, Current User, "Place all certificates in the following store",
  Trusted Root Certification Authorities. Firefox: see below.
- **macOS** (Safari, Chrome): open `ca.pem` in Keychain Access (login keychain), double-click it, Trust, "Always Trust". Firefox: see below.
- **Linux** (Chrome/Chromium): Settings, Privacy and security, Manage certificates, Authorities, Import `ca.pem`, tick "Trust for websites".
  System-wide: copy it to `/usr/local/share/ca-certificates/hams-node.crt` and run `sudo update-ca-certificates`.
- **Firefox** (any system): Settings, Privacy & Security, View Certificates, Authorities, Import, tick "Trust this CA to identify websites".
- **Android**: Settings, Security, Encryption and credentials, Install a certificate, CA certificate, pick `ca.pem`. Chrome on Android trusts it;
  Firefox for Android needs Settings, About, tap the logo five times, Secret Settings, "Use third party CA certificates".

Then tell the browser to use the node's PAC file, `http://ROUTER/hams.pac` (Windows/macOS/Linux/Android: the system proxy setting "Use setup
script"; Firefox: Settings, Network Settings, Automatic proxy configuration URL). Only `.ham` names, ham-network addresses and whitelisted
sites use the proxy; everything else goes direct. If your DHCP server offers option 252 as `http://ROUTER/wpad.dat`, browsers find it by themselves.
If the whitelist changes the node issues a new CA (`hams-node-proxyca ca --renew`); install the new certificate the same way.
