# Elecraft K4 attenuator and preamp

### `fn k4_front_end(&self, op: &Op) -> Option<Result<Request, RigError>>` [@ANCHOR: radioctl:radioctl_elecraft_k4_front_end]

Encodes the K4 attenuator (`RA`, dB and on flag) and preamp (`PA`, step and on flag) operations. These forms exist in the K4 meta mode K41 only, so the request is `K41;<command>;K4<n>;` in one write, where `n` is the meta mode the engine last saw (`K4n;` answers, asked for by `link_restored`; 0 until known). Values must be steps the description lists, else `Unsupported`. Returns `None` for any other operation. No I/O; unverified on a radio.
