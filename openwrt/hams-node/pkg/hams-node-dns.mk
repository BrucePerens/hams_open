# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-dns
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net .ham name forwarding for hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node +uci
  PKGARCH:=all
endef

define Package/hams-node-dns/description
  Writes a dnsmasq snippet that forwards only .ham and the member /40 reverse zone to the hams.com Net resolvers.
endef

define Package/hams-node-dns/conffiles
/etc/config/hams_node_dns
endef

define Package/hams-node-dns/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-dns $(1)/usr/sbin/
	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./files/etc/init.d/hams-node-dns $(1)/etc/init.d/
	$(INSTALL_CONF) ./files/etc/config/hams_node_dns $(1)/etc/config/
endef
