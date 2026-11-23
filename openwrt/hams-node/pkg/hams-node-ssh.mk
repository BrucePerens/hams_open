# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-ssh
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com key-only SSH for an enrolled hams-node
  URL:=https://hams.com
  DEPENDS:=+hams-node-enroll +dropbear +uci
  PKGARCH:=all
endef

define Package/hams-node-ssh/description
  Once the node is enrolled and an authorized key is installed, dropbear accepts public keys only (no password, no root password).
endef

define Package/hams-node-ssh/install
	$(INSTALL_DIR) $(1)/usr/sbin
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-ssh $(1)/usr/sbin/
endef
