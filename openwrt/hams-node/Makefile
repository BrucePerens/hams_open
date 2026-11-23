# SPDX-License-Identifier: GPL-2.0-or-later
# hams-node gateway package for AREDN nodes (OpenWrt 25.12 based). Built with the OpenWrt SDK
# (see README.md); installed from the node's admin package store (Upload Package), not with opkg.
include $(TOPDIR)/rules.mk

PKG_NAME:=hams-node
PKG_VERSION:=0.1.0
PKG_RELEASE:=1
PKG_LICENSE:=GPL-2.0-or-later

include $(INCLUDE_DIR)/package.mk

define Package/hams-node
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net gateway (Jool SIIT and WireGuard peers) for AREDN
  URL:=https://hams.com
  DEPENDS:=+jool-tools-siit +kmod-jool-siit +wireguard-tools +kmod-wireguard +jsonfilter +uclient-fetch
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

define Build/Compile
endef

define Package/hams-node/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/init.d $(1)/etc/config
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-siit $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-wg $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-ondemand $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/etc/init.d/hams-node-ondemand $(1)/etc/init.d/
	$(INSTALL_CONF) ./files/etc/config/hams_node_ondemand $(1)/etc/config/
	$(INSTALL_BIN) ./files/etc/init.d/hams-node $(1)/etc/init.d/
	$(INSTALL_CONF) ./files/etc/config/hams_node $(1)/etc/config/
endef

define Package/hams-node-enroll
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net enrolment and configuration renewal for hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node +wireguard-tools +jsonfilter +uclient-fetch +uci
  PKGARCH:=all
endef

define Package/hams-node-enroll/description
  Enrols the node with hams.com by a device code the member approves in the portal (or a one-time token),
  keeps only a revocable device token in a root-only file, and renews the hams-node configuration from cron.
endef

define Package/hams-node-enroll/conffiles
/etc/config/hams_node_enroll
endef

define Package/hams-node-enroll/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-enroll $(1)/usr/sbin/
	$(INSTALL_CONF) ./files/etc/config/hams_node_enroll $(1)/etc/config/
endef

$(eval $(call BuildPackage,hams-node))
$(eval $(call BuildPackage,hams-node-enroll))
