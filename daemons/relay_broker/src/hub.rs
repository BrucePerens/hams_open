// SPDX-License-Identifier: AGPL-3.0-or-later
//! The stand-alone emergency hub's settings and the `hostapd` and `dnsmasq` configuration written from fixed templates (RELAY_STANDALONE_EMERGENCY_HUB.md,
//! phase 1). Pure text generation. A `hub_start` request carries the validated settings; the broker writes these files itself and the relay never does.

use std::net::Ipv4Addr;

/// The owner's hub settings.
// [@ANCHOR: hub_hub_settings_struct]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubSettings {
    /// The wifi interface the access point uses, such as `wlan0`.
    pub interface: String,
    /// The network name, 1 to 32 printable ASCII characters, for example `HAMS-EMERGENCY-K6BP`.
    pub ssid: String,
    /// The WPA2 passphrase, 8 to 63 printable ASCII characters. The hub is never open.
    pub passphrase: String,
    /// 2.4 GHz channel 1 to 13, or a 5 GHz channel (36 to 165) when `five_ghz`.
    pub channel: u8,
    /// True for the 5 GHz band.
    pub five_ghz: bool,
    /// The hub's address; the network is its /24.
    pub hub_address: Ipv4Addr,
    /// The relay's DNS name, a single label such as `relay`; it resolves to the hub address.
    pub relay_name: String,
    /// The uplink's resolver, when there is an uplink; without one names other than the hub's fail at once.
    pub uplink_dns: Option<Ipv4Addr>,
    /// Lease time in minutes (default 60).
    pub lease_minutes: u32,
}

/// True when every byte is printable ASCII.
fn printable(text: &str) -> bool {
    text.bytes().all(|b| (0x20..0x7F).contains(&b))
}

// [@ANCHOR: relay_broker_hub_validate]
/// Checks the settings; the error says which one is wrong. Free text is limited to printable ASCII, so a value cannot add a line to a configuration file.
pub fn validate(s: &HubSettings) -> Result<(), String> {
    let interface_ok = !s.interface.is_empty()
        && s.interface.len() <= 15
        && s.interface
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.');
    if !interface_ok {
        return Err("the wifi interface name is not a plain interface name".into());
    }
    if s.ssid.is_empty() || s.ssid.len() > 32 || !printable(&s.ssid) {
        return Err("the network name must be 1 to 32 printable ASCII characters".into());
    }
    if s.passphrase.len() < 8 || s.passphrase.len() > 63 || !printable(&s.passphrase) {
        return Err(
            "the passphrase must be 8 to 63 printable ASCII characters (the hub is never open)"
                .into(),
        );
    }
    let channel_ok = if s.five_ghz {
        (36..=165).contains(&s.channel)
    } else {
        (1..=13).contains(&s.channel)
    };
    if !channel_ok {
        return Err("the channel is not valid for the chosen band".into());
    }
    let a = s.hub_address;
    if !(a.is_private() || a.is_link_local()) || a.octets()[3] == 0 || a.octets()[3] == 255 {
        return Err("the hub address must be a private host address".into());
    }
    let name_ok = !s.relay_name.is_empty()
        && s.relay_name.len() <= 63
        && s.relay_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !name_ok {
        return Err("the relay name must be one DNS label of letters, digits and hyphens".into());
    }
    if !(1..=24 * 60).contains(&s.lease_minutes) {
        return Err("the lease time must be 1 minute to 24 hours".into());
    }
    Ok(())
}

/// The first address of a /24 in 192.168.x.0 that does not overlap the uplink's network (given by any address on it), starting at 192.168.50.0 so it also
/// avoids the common router defaults (0, 1, 88, 100).
pub fn choose_hub_address(uplink: Option<Ipv4Addr>) -> Ipv4Addr {
    let taken = uplink
        .map(|u| u.octets())
        .filter(|o| o[0] == 192 && o[1] == 168);
    (50..=250u8)
        .find(|third| taken.is_none_or(|o| o[2] != *third))
        .map_or(Ipv4Addr::new(192, 168, 50, 1), |third| {
            Ipv4Addr::new(192, 168, third, 1)
        })
}

/// The `hostapd.conf` text: WPA2-PSK access point, AP-level carrying only (client isolation off so members and browsers reach each other).
pub fn hostapd_conf(s: &HubSettings) -> Result<String, String> {
    validate(s)?;
    Ok(format!(
        "# Written by the hams relay broker from fixed settings. Do not edit.\n\
         interface={}\n\
         driver=nl80211\n\
         ssid={}\n\
         hw_mode={}\n\
         channel={}\n\
         ieee80211n=1\n\
         wmm_enabled=1\n\
         ap_isolate=0\n\
         auth_algs=1\n\
         wpa=2\n\
         wpa_key_mgmt=WPA-PSK\n\
         rsn_pairwise=CCMP\n\
         wpa_passphrase={}\n",
        s.interface,
        s.ssid,
        if s.five_ghz { "a" } else { "g" },
        s.channel,
        s.passphrase
    ))
}

// [@ANCHOR: relay_broker_hub_dnsmasq_conf]
/// The `dnsmasq.conf` text: DHCP for the /24 (hub at the given address, pool .20 to .250), the hub as DNS server (option 6) and NTP server (option 42), the
/// relay's name and the OS connectivity-probe names resolving to the hub (so phones open the console instead of declaring "no internet"), and everything else
/// forwarded to the uplink resolver or, with none, refused at once.
pub fn dnsmasq_conf(s: &HubSettings) -> Result<String, String> {
    validate(s)?;
    let o = s.hub_address.octets();
    let net = |last: u8| Ipv4Addr::new(o[0], o[1], o[2], last);
    let forwarding = match s.uplink_dns {
        Some(dns) => format!("server={dns}\n"),
        None => "no-resolv\n".to_string(),
    };
    let probes = [
        "connectivitycheck.gstatic.com",
        "clients3.google.com",
        "captive.apple.com",
        "www.msftconnecttest.com",
        "detectportal.firefox.com",
    ]
    .map(|name| format!("address=/{name}/{}\n", s.hub_address))
    .concat();
    Ok(format!(
        "# Written by the hams relay broker from fixed settings. Do not edit.\n\
         interface={}\n\
         bind-interfaces\n\
         dhcp-range={},{},255.255.255.0,{}m\n\
         dhcp-option=3,{}\n\
         dhcp-option=6,{}\n\
         dhcp-option=42,{}\n\
         address=/{}/{}\n\
         address=/{}.local/{}\n\
         {forwarding}{probes}",
        s.interface,
        net(20),
        net(250),
        s.lease_minutes,
        s.hub_address,
        s.hub_address,
        s.hub_address,
        s.relay_name,
        s.hub_address,
        s.relay_name,
        s.hub_address,
    ))
}

/// Phase 0 takeover (RELAY_STANDALONE_EMERGENCY_HUB.md section 1a): the hub is a node of the user's own /24 and the router's DHCP is gone.
// [@ANCHOR: hub_takeover_settings_struct]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeoverSettings {
    /// The bridge (or Ethernet) interface on the user's LAN.
    pub interface: String,
    /// The hub's own fixed address on that LAN; the subnet is its /24.
    pub hub_address: Ipv4Addr,
    /// The router, handed out as the default gateway so nothing changes for clients when it comes back.
    pub gateway: Ipv4Addr,
    /// The relay's DNS name, one label.
    pub relay_name: String,
    /// The router's DNS server when it is still reachable; none means names other than the hub's fail at once.
    pub uplink_dns: Option<Ipv4Addr>,
}

/// Takeover leases are short so clients move back to the router quickly after the hand-back, minutes.
pub const TAKEOVER_LEASE_MINUTES: u32 = 5;

// [@ANCHOR: relay_broker_hub_takeover_dnsmasq_conf]
/// The `dnsmasq.conf` text for a takeover: DHCP for the same /24 from a high pool (.200 to .250, away from the router's own and static addresses),
/// short leases, the router as gateway, the hub as DNS and NTP server, the relay's own name. dnsmasq checks an address with a ping before it offers it
/// (we leave that on), which keeps it from handing out one that a still-running machine holds.
pub fn takeover_dnsmasq_conf(t: &TakeoverSettings) -> Result<String, String> {
    let hub = HubSettings {
        interface: t.interface.clone(),
        ssid: "x".into(),
        passphrase: "xxxxxxxx".into(),
        channel: 1,
        five_ghz: false,
        hub_address: t.hub_address,
        relay_name: t.relay_name.clone(),
        uplink_dns: t.uplink_dns,
        lease_minutes: TAKEOVER_LEASE_MINUTES,
    };
    validate(&hub)?;
    let o = t.hub_address.octets();
    let g = t.gateway.octets();
    if g[..3] != o[..3] || g[3] == 0 || g[3] == 255 || t.gateway == t.hub_address {
        return Err("the gateway must be another host address on the hub's /24".into());
    }
    if (200..=250).contains(&o[3]) || (200..=250).contains(&g[3]) {
        return Err(
            "the hub and the gateway must be outside the takeover pool .200 to .250".into(),
        );
    }
    let net = |last: u8| Ipv4Addr::new(o[0], o[1], o[2], last);
    let forwarding = match t.uplink_dns {
        Some(dns) => format!("server={dns}\n"),
        None => "no-resolv\n".to_string(),
    };
    Ok(format!(
        "# Written by the hams relay broker for a DHCP takeover. Do not edit.\n\
         interface={}\n\
         bind-interfaces\n\
         dhcp-range={},{},255.255.255.0,{}m\n\
         dhcp-option=3,{}\n\
         dhcp-option=6,{}\n\
         dhcp-option=42,{}\n\
         address=/{}/{}\n\
         address=/{}.local/{}\n\
         {forwarding}",
        t.interface,
        net(200),
        net(250),
        TAKEOVER_LEASE_MINUTES,
        t.gateway,
        t.hub_address,
        t.hub_address,
        t.relay_name,
        t.hub_address,
        t.relay_name,
        t.hub_address,
    ))
}

#[cfg(test)]
mod tests {
    // Tests [@ANCHOR: hub_hub_settings_struct]
    // Tests [@ANCHOR: hub_takeover_settings_struct]
    use super::*;

    fn good() -> HubSettings {
        HubSettings {
            interface: "wlan0".into(),
            ssid: "HAMS-EMERGENCY-K6BP".into(),
            passphrase: "correct horse".into(),
            channel: 6,
            five_ghz: false,
            hub_address: Ipv4Addr::new(192, 168, 50, 1),
            relay_name: "relay".into(),
            uplink_dns: None,
            lease_minutes: 60,
        }
    }
    // Tests [@ANCHOR: relay_broker_hub_validate]
    #[test]
    fn settings_that_could_add_a_line_or_leave_the_hub_open_are_refused() {
        assert_eq!(validate(&good()), Ok(()));
        let mutate = |f: fn(&mut HubSettings)| {
            let mut s = good();
            f(&mut s);
            validate(&s).is_err()
        };
        assert!(mutate(|s| s.ssid = "a\nwpa=0".into()));
        assert!(mutate(|s| s.passphrase = "short".into()));
        assert!(mutate(|s| s.passphrase = String::new()));
        assert!(mutate(|s| s.passphrase = "p\nassword1".into()));
        assert!(mutate(|s| s.interface = "wlan0;reboot".into()));
        assert!(mutate(|s| s.channel = 14));
        assert!(mutate(|s| s.relay_name = "re lay".into()));
        assert!(mutate(|s| s.hub_address = Ipv4Addr::new(8, 8, 8, 8)));
        assert!(mutate(|s| s.lease_minutes = 0));
        assert!(!mutate(|s| {
            s.five_ghz = true;
            s.channel = 36
        }));
    }
    #[test]
    fn the_hub_subnet_avoids_the_uplinks_network() {
        assert_eq!(choose_hub_address(None), Ipv4Addr::new(192, 168, 50, 1));
        assert_eq!(
            choose_hub_address(Some(Ipv4Addr::new(192, 168, 50, 77))),
            Ipv4Addr::new(192, 168, 51, 1)
        );
        assert_eq!(
            choose_hub_address(Some(Ipv4Addr::new(10, 0, 0, 5))),
            Ipv4Addr::new(192, 168, 50, 1)
        );
    }
    #[test]
    fn the_access_point_config_is_wpa2_and_lets_members_reach_each_other() {
        let text = hostapd_conf(&good()).unwrap();
        for line in [
            "interface=wlan0",
            "ssid=HAMS-EMERGENCY-K6BP",
            "hw_mode=g",
            "channel=6",
            "ap_isolate=0",
            "wpa=2",
            "wpa_passphrase=correct horse",
        ] {
            assert!(text.lines().any(|l| l == line), "{line}\n{text}");
        }
        let mut five = good();
        five.five_ghz = true;
        five.channel = 40;
        assert!(hostapd_conf(&five).unwrap().contains("hw_mode=a"));
        let mut open = good();
        open.passphrase.clear();
        assert!(hostapd_conf(&open).is_err());
    }
    // Tests [@ANCHOR: relay_broker_hub_dnsmasq_conf]
    #[test]
    fn the_dhcp_and_dns_config_points_clients_at_the_hub_for_time_names_and_probes() {
        let text = dnsmasq_conf(&good()).unwrap();
        for line in [
            "dhcp-range=192.168.50.20,192.168.50.250,255.255.255.0,60m",
            "dhcp-option=6,192.168.50.1",
            "dhcp-option=42,192.168.50.1",
            "address=/relay/192.168.50.1",
            "address=/captive.apple.com/192.168.50.1",
            "no-resolv",
        ] {
            assert!(text.lines().any(|l| l == line), "{line}\n{text}");
        }
        let mut up = good();
        up.uplink_dns = Some(Ipv4Addr::new(192, 168, 1, 1));
        let text = dnsmasq_conf(&up).unwrap();
        assert!(text.lines().any(|l| l == "server=192.168.1.1") && !text.contains("no-resolv"));
    }

    fn takeover() -> TakeoverSettings {
        TakeoverSettings {
            interface: "br0".into(),
            hub_address: Ipv4Addr::new(192, 168, 1, 5),
            gateway: Ipv4Addr::new(192, 168, 1, 1),
            relay_name: "relay".into(),
            uplink_dns: None,
        }
    }
    // Tests [@ANCHOR: relay_broker_hub_takeover_dnsmasq_conf]
    #[test]
    fn the_takeover_serves_the_same_subnet_from_a_high_pool_with_short_leases() {
        let text = takeover_dnsmasq_conf(&takeover()).unwrap();
        for line in [
            "interface=br0",
            "dhcp-range=192.168.1.200,192.168.1.250,255.255.255.0,5m",
            "dhcp-option=3,192.168.1.1",
            "dhcp-option=6,192.168.1.5",
            "dhcp-option=42,192.168.1.5",
            "address=/relay/192.168.1.5",
            "no-resolv",
        ] {
            assert!(text.lines().any(|l| l == line), "{line}\n{text}");
        }
        assert!(!text.contains("no-ping"), "the ping check stays on");
        let mut up = takeover();
        up.uplink_dns = Some(Ipv4Addr::new(192, 168, 1, 1));
        let text = takeover_dnsmasq_conf(&up).unwrap();
        assert!(text.lines().any(|l| l == "server=192.168.1.1") && !text.contains("no-resolv"));
    }
    #[test]
    fn a_takeover_gateway_off_the_subnet_or_a_hub_in_the_pool_is_refused() {
        let bad = |f: fn(&mut TakeoverSettings)| {
            let mut t = takeover();
            f(&mut t);
            takeover_dnsmasq_conf(&t).is_err()
        };
        assert!(bad(|t| t.gateway = Ipv4Addr::new(192, 168, 2, 1)));
        assert!(bad(|t| t.gateway = t.hub_address));
        assert!(bad(|t| t.hub_address = Ipv4Addr::new(192, 168, 1, 210)));
        assert!(bad(|t| t.gateway = Ipv4Addr::new(192, 168, 1, 250)));
        assert!(bad(|t| t.interface = "br0;x".into()));
    }
}
