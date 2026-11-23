# SPDX-License-Identifier: GPL-2.0-or-later
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
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-key $(1)/usr/sbin/
	$(INSTALL_CONF) ./files/etc/config/hams_node_enroll $(1)/etc/config/
endef
