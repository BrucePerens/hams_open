# SPDX-License-Identifier: GPL-2.0-or-later
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
