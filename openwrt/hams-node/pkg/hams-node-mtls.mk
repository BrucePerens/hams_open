# SPDX-License-Identifier: GPL-2.0-or-later
define Package/hams-node-mtls
  SECTION:=net
  CATEGORY:=Network
  TITLE:=hams.com member client certificate required for router management
  URL:=https://hams.com
  DEPENDS:=+hams-node-enroll +nginx-ssl +openssl-util +uhttpd +rpcd +jsonfilter +ubus +uci +uclient-fetch
  PKGARCH:=all
endef

define Package/hams-node-mtls/description
  Puts nginx (mutual TLS) in front of uhttpd: LuCI, ubus over HTTP and the hams-node APIs are reachable only with a hams.com
  member certificate that chains to the pinned member CA and carries the enrolled owner's callsign.
endef

define Package/hams-node-mtls/conffiles
/etc/hams-node/member-ca.pem
/etc/hams-node/member-crls.pem
endef

define Package/hams-node-mtls/install
	$(INSTALL_DIR) $(1)/usr/lib/hams-node/sso/cgi-bin $(1)/usr/sbin $(1)/etc/init.d $(1)/etc/hams-node
	$(INSTALL_BIN) ./files/usr/sbin/hams-node-mtls $(1)/usr/sbin/
	$(INSTALL_BIN) ./files/usr/lib/hams-node/sso/cgi-bin/sso $(1)/usr/lib/hams-node/sso/cgi-bin/
	$(INSTALL_BIN) ./files/etc/init.d/hams-node-mtls $(1)/etc/init.d/
	$(INSTALL_DATA) ./files/etc/hams-node/member-ca.pem $(1)/etc/hams-node/
	$(INSTALL_DATA) ./files/etc/hams-node/member-crls.pem $(1)/etc/hams-node/
endef
