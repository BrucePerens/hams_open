# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net gateway (Jool SIIT and WireGuard peers) for AREDN
  URL:=https://hams.com
  DEPENDS:=+jool-tools-netfilter +kmod-jool-netfilter +wireguard-tools +kmod-wireguard +jsonfilter +uclient-fetch +usign
  PKGARCH:=all
endef

define Package/hams-node/description
  Translates one AREDN mesh into the hams.com Net IPv4 translation block (stateless SIIT, one /96 per
  domain from the registry export) and adds WireGuard peers to hams.com route servers over Internet
  links only. It does not change AREDN's own services, routing or firewall zones.
endef

define Package/hams-node/conffiles
/etc/config/hams_node
/etc/config/hams_node_ondemand
endef

define Package/hams-node/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/init.d $(1)/etc/config $(1)/etc/hams-node
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-siit $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-wg $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-rf-route $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-ondemand $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-verify $(1)/usr/sbin/
	$(INSTALL_DATA) ./files/etc/hams-node/pinned_keys $(1)/etc/hams-node/
	$(INSTALL_BIN) ./files/etc/init.d/hams-node-ondemand $(1)/etc/init.d/
	$(INSTALL_CONF) ./files/etc/config/hams_node_ondemand $(1)/etc/config/
	$(INSTALL_BIN) ./files/etc/init.d/hams-node $(1)/etc/init.d/
	$(INSTALL_CONF) ./files/etc/config/hams_node $(1)/etc/config/
endef
