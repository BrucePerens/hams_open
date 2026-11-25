# SPDX-License-Identifier: GPL-2.0-or-later
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

define Package/hams-node-traffic/prerm
#!/bin/sh
[ -n "$$IPKG_INSTROOT" ] || /usr/sbin/hams-node-traffic stop
exit 0
endef

define Package/hams-node-traffic/install
	$(INSTALL_DIR) $(1)/usr/sbin
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-traffic $(1)/usr/sbin/
endef
