# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-status
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com Net remote switch-off and rogue-router warnings for hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node-enroll +jsonfilter +uclient-fetch +usign +wireguard-tools
  PKGARCH:=all
endef

define Package/hams-node-status/description
  Polls hams.com every two minutes for a signed status. A signed "disabled" for this device brings WireGuard down, removes the
  Hams.com Net routes and stops the daemons (configuration kept, reason shown in LuCI); a signed "ok" turns them back on. Any
  network failure or unsigned reply changes nothing. Also hands the signed list to neighbouring routers (tcp 5710) and takes a newer one
  from them, so a warning spreads with hams.com unreachable. Also keeps the signed deprecation list (rogue routers) and blocks what it names.
endef

define Package/hams-node-status/install
	$(INSTALL_DIR) $(1)/usr/sbin $(1)/etc/init.d
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-status $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/etc/init.d/hams-node-status $(1)/etc/init.d/
endef
