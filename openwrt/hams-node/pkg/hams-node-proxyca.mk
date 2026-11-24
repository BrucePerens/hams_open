# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-proxyca
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net proxy: per-node name-constrained CA and PAC file for browsers
  URL:=https://hams.com
  DEPENDS:=+openssl-util +uci
  PKGARCH:=all
endef

define Package/hams-node-proxyca/description
  Makes the node's own CA (private key stays on the router, certificate constrained to .ham and the whitelisted domains) and the PAC/WPAD
  file that sends only .ham names, ham-network addresses and whitelisted sites to the proxy. Install instructions: INSTALL_CA.md.
endef

define Package/hams-node-proxyca/conffiles
/etc/config/hams_node_proxyca
endef

define Package/hams-node-proxyca/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config $(1)/usr/share/doc/hams-node-proxyca
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-proxyca $(1)/usr/sbin/
	$(INSTALL_CONF) ./files/etc/config/hams_node_proxyca $(1)/etc/config/
	$(INSTALL_DATA) ./INSTALL_CA.md $(1)/usr/share/doc/hams-node-proxyca/
endef
