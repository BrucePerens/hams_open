// SPDX-License-Identifier: AGPL-3.0-or-later
//! Serves a [`VirtualFlex`] on a real TCP listener, so the engine, the rig actor and the TCP
//! transport can be tested against something that behaves like a radio on port 4992 (the caller
//! binds the listener, normally to `127.0.0.1:0`). One client at a time; a new connection
//! replaces the previous one and gets the `V`/`H` prologue.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::mpsc;

use crate::virtual_flex::{FlexPanel, FlexRigState, Order, VirtualFlex};

struct Shared {
    rig: Mutex<VirtualFlex>,
    push: mpsc::UnboundedSender<Vec<u8>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Control of a served virtual radio.
#[derive(Clone)]
pub struct VirtualFlexControl {
    shared: Arc<Shared>,
}

// [@ANCHOR: radioctl_virtual_flex_serve]
/// Serves `rig` on `listener` from a task on `runtime`.
pub fn serve_virtual_flex(
    runtime: &Handle,
    listener: TcpListener,
    rig: VirtualFlex,
) -> VirtualFlexControl {
    let (push, mut pushed) = mpsc::unbounded_channel::<Vec<u8>>();
    let shared = Arc::new(Shared {
        rig: Mutex::new(rig),
        push,
    });
    let task_shared = Arc::clone(&shared);
    runtime.spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let _ = stream.set_nodelay(true);
            while pushed.try_recv().is_ok() {}
            let greeting = lock(&task_shared.rig).greeting();
            if stream.write_all(&greeting).await.is_err() {
                continue;
            }
            let mut buffer = [0u8; 512];
            loop {
                tokio::select! {
                    read = stream.read(&mut buffer) => {
                        let Ok(n) = read else { break };
                        if n == 0 {
                            break;
                        }
                        let reply = lock(&task_shared.rig)
                            .feed(buffer.get(..n).unwrap_or(&[]), Duration::ZERO);
                        if !reply.is_empty() && stream.write_all(&reply).await.is_err() {
                            break;
                        }
                    }
                    Some(bytes) = pushed.recv() => {
                        if stream.write_all(&bytes).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });
    VirtualFlexControl { shared }
}

impl VirtualFlexControl {
    // [@ANCHOR: radioctl_virtual_flex_control_state]
    /// A copy of the radio's state.
    pub fn state(&self) -> FlexRigState {
        lock(&self.shared.rig).state().clone()
    }

    // [@ANCHOR: radioctl_virtual_flex_control_forget_session]
    /// The radio forgets this client's subscriptions (the client reconnected).
    pub fn forget_session(&self) {
        lock(&self.shared.rig).forget_session();
    }

    // [@ANCHOR: radioctl_virtual_flex_control_subscribed]
    /// Whether the client is subscribed to slice, tx and pan status.
    pub fn subscribed(&self) -> (bool, bool, bool) {
        lock(&self.shared.rig).subscribed()
    }

    /// Every command line the radio has received, in order.
    pub fn received(&self) -> Vec<String> {
        lock(&self.shared.rig).received().to_vec()
    }

    /// Chooses whether status follows or precedes the reply that caused it.
    pub fn set_order(&self, order: Order) {
        lock(&self.shared.rig).order = order;
    }

    // [@ANCHOR: radioctl_virtual_flex_control_front_panel]
    /// Changes something at the radio; status goes to the connected client at once.
    pub fn front_panel(&self, change: FlexPanel) {
        let bytes = lock(&self.shared.rig).front_panel(change);
        if !bytes.is_empty() {
            let _ = self.shared.push.send(bytes);
        }
    }
}
