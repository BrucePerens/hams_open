# SPDX-License-Identifier: GPL-2.0-or-later
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
