# SPDX-License-Identifier: GPL-2.0-or-later
# ham_proxy for the stock AREDN image of one target: the static musl binary, built with the host's cargo against the SDK's cross compiler
# (the OpenWrt SDK has no rust). Needs `rustup target add <RUST_TARGET>` for the node's CPU (x86_64-unknown-linux-musl,
# aarch64-unknown-linux-musl, armv7-unknown-linux-musleabihf; mips targets need a nightly build-std toolchain). See ../hams-node/README.md.
include $(TOPDIR)/rules.mk

PKG_NAME:=ham-proxy
PKG_VERSION:=0.1.0
PKG_RELEASE:=1
PKG_LICENSE:=AGPL-3.0-or-later

include $(INCLUDE_DIR)/package.mk

# HAMS_OPEN_TREE: the hams_open checkout; RUST_TARGET: the Rust triple of the node's CPU.
HAMS_OPEN_TREE?=$(CURDIR)/../..
RUST_TARGET?=x86_64-unknown-linux-musl
CARGO_LINKER=$(TARGET_CC_NOCACHE)

define Package/ham-proxy
  SECTION:=net
  CATEGORY:=Network
  TITLE:=ham_proxy, the integrity-only TLS web proxy of hams.com Net
  URL:=https://hams.com
  DEPENDS:=
endef

define Package/ham-proxy/description
  Integrity-only TLS toward the ham side (authenticated, never encrypted, as Part 97 requires on the air), ordinary verified TLS toward
  the site. Static binary, OpenSSL built in.
endef

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
endef

define Build/Compile
	cd $(HAMS_OPEN_TREE)/daemons/ham_proxy && \
	CC_$(subst -,_,$(RUST_TARGET))="$(CARGO_LINKER)" \
	CARGO_TARGET_$(shell echo $(RUST_TARGET) | tr 'a-z-' 'A-Z_')_LINKER="$(CARGO_LINKER)" \
	cargo build --release --locked --features vendored --target $(RUST_TARGET) --target-dir $(PKG_BUILD_DIR)/target
endef

define Package/ham-proxy/install
	$(INSTALL_DIR) $(1)/usr/sbin
	$(INSTALL_BIN) $(PKG_BUILD_DIR)/target/$(RUST_TARGET)/release/ham_proxy $(1)/usr/sbin/
endef

$(eval $(call BuildPackage,ham-proxy))
