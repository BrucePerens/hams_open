// SPDX-License-Identifier: AGPL-3.0-or-later
//! Band following: a selector (an antenna switch, a band-decoder output or an amplifier's band
//! input) follows the frequency the rig will transmit on, not the receive frequency.
//!
//! The source is `Field::ActiveTxFrequency` of the rig's status-change stream, which is correct in
//! split operation and while keyed (Hamlib issue 138: an amplifier that followed the CAT frequency
//! did not see the transmit frequency). The switch itself goes through [`Station::select`], so the
//! keyed-state interlock still applies: a band change that arrives while the rig is keyed is
//! refused, and the follower tries again on the next event (the unkey is one).

use crate::error::StationError;
use crate::model::ChannelId;
use crate::station::Station;
use futures_core::Stream;
use radioctl_core::op::Field;
use radioctl_core::rig::{Freshness, RigHandle};
use radioctl_core::units::Hz;
use std::future::poll_fn;
use std::pin::Pin;

// [@ANCHOR: radioctl_station_band_follow_band_route]
/// One band: transmit frequencies from `low` to `high` inclusive select `port` of `channel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BandRoute {
    /// Lowest frequency of the band.
    pub low: Hz,
    /// Highest frequency of the band.
    pub high: Hz,
    /// The selector channel.
    pub channel: ChannelId,
    /// The way (from 1) to select.
    pub port: u32,
}

// [@ANCHOR: radioctl_station_band_route_for]
/// The first route whose band holds `hz`, or `None` outside every band (the selector is then left alone).
pub fn route_for(routes: &[BandRoute], hz: Hz) -> Option<&BandRoute> {
    routes.iter().find(|r| r.low <= hz && hz <= r.high)
}

// [@ANCHOR: radioctl_station_band_follow_tx_frequency]
/// Follows the rig's transmit frequency until its event stream ends: switches the selector of the
/// matching route, behind the station's interlock, once at start and after every event, and only
/// when the wanted way is not the one last applied. A refused or failed switch is passed to
/// `on_result` and retried on the next event.
pub async fn follow_tx_frequency(
    station: &Station,
    rig: &RigHandle,
    routes: &[BandRoute],
    mut on_result: impl FnMut(&BandRoute, &Result<(), StationError>),
) {
    let mut events = rig.events();
    let mut applied: Option<(ChannelId, u32)> = None;
    loop {
        if let Ok(hz) = rig.active_tx_frequency(Freshness::Cached).await {
            if let Some(route) = route_for(routes, hz) {
                let want = (route.channel.clone(), route.port);
                if applied.as_ref() != Some(&want) {
                    let result = station.select(&route.channel, route.port).await;
                    if result.is_ok() {
                        applied = Some(want);
                    }
                    on_result(route, &result);
                }
            }
        }
        let next = poll_fn(|cx| Pin::new(&mut events).poll_next(cx)).await;
        match next {
            Some(event) => {
                // Only a change that can move the transmit frequency (or the keyed state a refused
                // switch waited for) needs another look.
                let relevant = event
                    .changes
                    .iter()
                    .any(|c| matches!(c.field, Field::ActiveTxFrequency | Field::Ptt));
                if !relevant {
                    continue;
                }
            }
            None => return,
        }
    }
}

// [@ANCHOR: radioctl_station_follow_configured]
/// Starts the follower for the rig named `rig_name` from the station's own `[[band_routes]]`:
/// only the channels that belong to this rig and have not opted out follow (see
/// [`crate::model::StationDef::band_routes_for`]). Returns at once when the rig has no routes.
pub async fn follow_configured(
    station: &Station,
    rig_name: &str,
    rig: &RigHandle,
    on_result: impl FnMut(&BandRoute, &Result<(), StationError>),
) {
    let routes = station.def().band_routes_for(rig_name);
    if routes.is_empty() {
        return;
    }
    follow_tx_frequency(station, rig, &routes, on_result).await;
}
