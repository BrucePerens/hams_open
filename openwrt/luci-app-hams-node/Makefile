# SPDX-License-Identifier: GPL-2.0-or-later
# LuCI status and settings page for the hams-node package (OpenWrt 25.12, client-side JS view + rpcd exec plugin).
# Written against package.mk so it builds in an SDK without the luci feed; with the luci feed it could use luci.mk instead.
include $(TOPDIR)/rules.mk

PKG_NAME:=luci-app-hams-node
PKG_VERSION:=0.1.0
PKG_RELEASE:=1
PKG_LICENSE:=GPL-2.0-or-later

include $(INCLUDE_DIR)/package.mk

define Package/luci-app-hams-node
  SECTION:=luci
  CATEGORY:=LuCI
  SUBMENU:=3. Applications
  TITLE:=LuCI page for the hams.com Net node (hams-node)
  URL:=https://hams.com
  DEPENDS:=+hams-node +luci-base +rpcd
  PKGARCH:=all
endef

define Build/Compile
endef

define Package/luci-app-hams-node/install
	$(INSTALL_DIR) $(1)/www/luci-static/resources/view/hams-node $(1)/usr/libexec/rpcd $(1)/usr/share/luci/menu.d $(1)/usr/share/rpcd/acl.d
	$(INSTALL_DATA) ./htdocs/luci-static/resources/view/hams-node/status.js.in $(1)/www/luci-static/resources/view/hams-node/status.js
	$(INSTALL_BIN) ./root/usr/libexec/rpcd/hams-node $(1)/usr/libexec/rpcd/
	$(INSTALL_DATA) ./root/usr/share/luci/menu.d/luci-app-hams-node.json $(1)/usr/share/luci/menu.d/
	$(INSTALL_DATA) ./root/usr/share/rpcd/acl.d/luci-app-hams-node.json $(1)/usr/share/rpcd/acl.d/
endef

$(eval $(call BuildPackage,luci-app-hams-node))
