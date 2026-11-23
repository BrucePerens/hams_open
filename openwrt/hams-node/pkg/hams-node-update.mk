# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-update
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net signed-feed automatic update for hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node +uci
  PKGARCH:=all
endef

define Package/hams-node-update/description
  Installs newer hams-node packages from the signed hams.com apk feed (staged, checked, rolled back on failure).
  Does nothing until a feed URL and the feed's public key are configured in /etc/config/hams_node_update.
endef

define Package/hams-node-update/conffiles
/etc/config/hams_node_update
endef

define Package/hams-node-update/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-update $(1)/usr/sbin/
	$(INSTALL_CONF) ./files/etc/config/hams_node_update $(1)/etc/config/
endef
