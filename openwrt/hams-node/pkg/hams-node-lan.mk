# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-lan
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net /60 delegation to LAN interfaces for hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node-enroll +odhcpd +uci
  PKGARCH:=all
endef

define Package/hams-node-lan/description
  Gives each LAN interface its own /64 of the member's /60 (netifd ip6assign) and serves it with odhcpd (RA and DHCPv6).
endef

define Package/hams-node-lan/conffiles
/etc/config/hams_node_lan
endef

define Package/hams-node-lan/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-lan $(1)/usr/sbin/
	$(INSTALL_CONF) ./files/etc/config/hams_node_lan $(1)/etc/config/
endef
