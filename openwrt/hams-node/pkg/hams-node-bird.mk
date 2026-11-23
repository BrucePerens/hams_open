# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-bird
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net BIRD 2 sessions for hams-node (multi-hop)
  URL:=https://hams.com
  DEPENDS:=+hams-node +bird2 +uci
  PKGARCH:=all
endef

define Package/hams-node-bird/description
  Renders the node's BIRD 2 configuration (a session to each hams.com route server, RF-first local preference) from
  /etc/config/hams_node_bird. Off by default.
endef

define Package/hams-node-bird/conffiles
/etc/config/hams_node_bird
endef

define Package/hams-node-bird/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-bird $(1)/usr/sbin/
	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./files/etc/init.d/hams-node-bird $(1)/etc/init.d/
	$(INSTALL_CONF) ./files/etc/config/hams_node_bird $(1)/etc/config/
endef
