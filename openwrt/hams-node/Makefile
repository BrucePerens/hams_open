# SPDX-License-Identifier: GPL-2.0-or-later
# hams-node gateway package for AREDN nodes (OpenWrt 25.12 based). Built with the OpenWrt SDK
# (see README.md); installed from the node's admin package store (Upload Package), not with opkg.
include $(TOPDIR)/rules.mk

PKG_NAME:=hams-node
PKG_VERSION:=0.1.0
PKG_RELEASE:=1
PKG_LICENSE:=GPL-2.0-or-later

include $(INCLUDE_DIR)/package.mk

define Build/Compile
endef

# Each subpackage lives in its own pkg/<name>.mk (its Package/<name> blocks only, see README.md); the package name is the
# file name. The BuildPackage calls are generated here, so a new subpackage touches no shared line.
SCAN_DEPS=pkg/*.mk
include $(sort $(wildcard $(CURDIR)/pkg/*.mk))
$(foreach p,$(sort $(basename $(notdir $(wildcard $(CURDIR)/pkg/*.mk)))),$(eval $(call BuildPackage,$(p))))
