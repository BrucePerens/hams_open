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

define Package/hams-node-time
  SECTION:=net
  CATEGORY:=Network
  TITLE:=Cross-checked clock sources for hams-node (NTP, ham-network NTP, GPS, WWV/WWVB, RTC)
  URL:=https://hams.com
  DEPENDS:=+hams-node +uci
  PKGARCH:=all
endef

define Package/hams-node-time/description
  Sets the clock from an ordered list of sources, never from one alone: a disagreeing source is flagged, large steps need a
  second agreeing source. Off by default.
endef

define Package/hams-node-time/conffiles
/etc/config/hams_node_time
endef

define Package/hams-node-time/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-time $(1)/usr/sbin/
	$(INSTALL_CONF) ./files/etc/config/hams_node_time $(1)/etc/config/
endef

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

define Package/hams-node-fw/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/config
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-fw $(1)/usr/sbin/
	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./files/etc/init.d/hams-node-fw $(1)/etc/init.d/
	$(INSTALL_CONF) ./files/etc/config/hams_node_fw $(1)/etc/config/
endef

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

$(eval $(call BuildPackage,hams-node))
$(eval $(call BuildPackage,hams-node-enroll))
$(eval $(call BuildPackage,hams-node-report))
$(eval $(call BuildPackage,hams-node-traffic))
$(eval $(call BuildPackage,hams-node-bird))
$(eval $(call BuildPackage,hams-node-fw))
$(eval $(call BuildPackage,hams-node-update))
$(eval $(call BuildPackage,hams-node-lan))
$(eval $(call BuildPackage,hams-node-dns))
$(eval $(call BuildPackage,hams-node-time))
