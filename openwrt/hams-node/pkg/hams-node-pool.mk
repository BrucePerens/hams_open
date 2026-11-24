# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-pool
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net IPv4 address pool for AREDN clients
  URL:=https://hams.com
  DEPENDS:=+hams-node +uci
  PKGARCH:=all
endef

define Package/hams-node-pool/description
  A small IPv4 pool on the AREDN side: each chosen .ham name gets a pool address that Jool SIIT maps to the host's Hams.com Net address, and the
  node's dnsmasq answers the name with it. Only listed unencrypted TCP ports reach the pool (Part 97); targets must lie inside the network prefix.
endef

define Package/hams-node-pool/conffiles
/etc/config/hams_node_pool
/etc/hams-node/pool.map
endef

define Package/hams-node-pool/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config $(1)/etc/init.d
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-pool $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/etc/init.d/hams-node-pool $(1)/etc/init.d/
	$(INSTALL_CONF) ./files/etc/config/hams_node_pool $(1)/etc/config/
endef
