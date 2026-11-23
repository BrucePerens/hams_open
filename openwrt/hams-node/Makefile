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
  DEPENDS:=+jool-tools-netfilter +kmod-jool-netfilter +wireguard-tools +kmod-wireguard +jsonfilter +uclient-fetch
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
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-rf-route $(1)/usr/sbin/
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

define Package/hams-node-report
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net AREDN host and service reporter for hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node-enroll +jsonfilter +uclient-fetch
  PKGARCH:=all
endef

define Package/hams-node-report/description
  Sends the node's AREDN host list and advertised services to hams.com from cron, authenticated by the enrolled
  device token, so they are published as .ham names.
endef

define Package/hams-node-report/install
	$(INSTALL_DIR) $(1)/usr/sbin
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-report $(1)/usr/sbin/
endef

define Package/hams-node-traffic
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net per-peer traffic totals for hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node-enroll +jsonfilter +uclient-fetch +wireguard-tools
  PKGARCH:=all
endef

define Package/hams-node-traffic/description
  Sends hams.com the bytes each WireGuard peer carried since the last report (counters only, no content or flows) so
  busy member pairs can move to radio links. `hams-node-traffic optout` stops it.
endef

define Package/hams-node-traffic/install
	$(INSTALL_DIR) $(1)/usr/sbin
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-traffic $(1)/usr/sbin/
endef

$(eval $(call BuildPackage,hams-node))
$(eval $(call BuildPackage,hams-node-enroll))
$(eval $(call BuildPackage,hams-node-report))
$(eval $(call BuildPackage,hams-node-traffic))
