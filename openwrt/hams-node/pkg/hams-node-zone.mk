# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-zone
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net .ham zone as a local secondary for hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node-dns +unbound-daemon +uci
  PKGARCH:=all
endef

define Package/hams-node-zone/description
  Keeps a copy of the .ham zone on the node (unbound auth-zone, AXFR); point hams-node-dns at it (resolver 127.0.0.1#5354) so
  names resolve with the anycast servers or the internet gone. Off by default.
endef

define Package/hams-node-zone/conffiles
/etc/config/hams_node_zone
endef

define Package/hams-node-zone/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-zone $(1)/usr/sbin/
	$(INSTALL_CONF) ./files/etc/config/hams_node_zone $(1)/etc/config/
endef
