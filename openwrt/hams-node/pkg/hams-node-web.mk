# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-web
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net web gateway for AREDN users (.ham pages over integrity-only TLS)
  URL:=https://hams.com
  DEPENDS:=+hams-node +ham-proxy +uci
  PKGARCH:=all
endef

define Package/hams-node-web/description
  Serves .ham pages (and whitelisted sites) to IPv4 clients on an AREDN mesh through ham_proxy: integrity-only TLS toward the mesh (never
  encrypted, Part 97), station-ID log, sites not on the whitelist refused, no connection to private addresses. Off until configured.
endef

define Package/hams-node-web/conffiles
/etc/config/hams_node_web
endef

define Package/hams-node-web/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config $(1)/etc/init.d $(1)/etc/hams-node/web
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-web $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/etc/init.d/hams-node-web $(1)/etc/init.d/
	$(INSTALL_CONF) ./files/etc/config/hams_node_web $(1)/etc/config/
	$(INSTALL_DATA) ./files/etc/hams-node/web/empty-whitelist.txt $(1)/etc/hams-node/web/
endef
