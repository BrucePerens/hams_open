# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-endpoint
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net signed WireGuard endpoint updates for hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node-enroll +openssl-util +jsonfilter +uclient-fetch +uci
  PKGARCH:=all
endef

define Package/hams-node-endpoint/description
  Publishes the node's WireGuard endpoint to hams.com, signed with the node's own key (the private key never leaves the router),
  when the WAN address changes (hotplug) and every 30 minutes (cron refresh).
endef

define Package/hams-node-endpoint/conffiles
/etc/config/hams_node_endpoint
endef

define Package/hams-node-endpoint/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config $(1)/etc/hotplug.d/iface
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-endpoint $(1)/usr/sbin/
	$(INSTALL_CONF) ./files/etc/config/hams_node_endpoint $(1)/etc/config/
	$(INSTALL_BIN) ./files/etc/hotplug.d/iface/90-hams-node-endpoint $(1)/etc/hotplug.d/iface/
endef

define Package/hams-node-endpoint/postinst
#!/bin/sh
[ -n "$$IPKG_INSTROOT" ] || /usr/sbin/hams-node-endpoint start
exit 0
endef

define Package/hams-node-endpoint/prerm
#!/bin/sh
[ -n "$$IPKG_INSTROOT" ] || /usr/sbin/hams-node-endpoint stop
exit 0
endef
