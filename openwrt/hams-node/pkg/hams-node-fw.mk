# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-fw
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net firewall zones and rf interface registry for hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node +uci
  PKGARCH:=all
endef

define Package/hams-node-fw/description
  Firewall zones for the ham network (inbound closed except ports the member opens; encrypted-service ports never toward RF)
  and the registry of radio-link interfaces on which hams-node-wg refuses WireGuard.
endef

define Package/hams-node-fw/conffiles
/etc/config/hams_node_fw
endef

define Package/hams-node-fw/prerm
#!/bin/sh
[ -n "$$IPKG_INSTROOT" ] || [ "$$1" != remove ] || { /usr/sbin/hams-node-fw remove && /etc/init.d/firewall reload; }
exit 0
endef

define Package/hams-node-fw/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-fw $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-rf $(1)/usr/sbin/
	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_DIR) $(1)/etc/hotplug.d/iface
	$(INSTALL_BIN) ./files/etc/hotplug.d/iface/90-hams-node-rf $(1)/etc/hotplug.d/iface/
	$(INSTALL_BIN) ./files/etc/init.d/hams-node-fw $(1)/etc/init.d/
	$(INSTALL_CONF) ./files/etc/config/hams_node_fw $(1)/etc/config/
endef
