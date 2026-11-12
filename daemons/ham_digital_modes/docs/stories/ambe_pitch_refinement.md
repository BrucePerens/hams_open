# Story: AMBE Encoder Pitch Refinement Synthetic Spectrum (ham_digital_modes)

## `S_w(m, omega0)` [@ANCHOR: ham_digital_modes:SyntheticSpectrum]

The float AMBE (TIA-102.BABA) encoder refines a half-sample-accuracy pitch estimate to quarter-sample
accuracy by trying candidate fundamental frequencies and scoring each against the real frame spectrum.
The scoring needs the synthetic spectrum `S_w(m, omega0)` (Eq. 25): for a DFT bin `m`, it finds which
harmonic band of the candidate `omega0` the bin falls into (Eq. 26-27) and returns that harmonic's estimated
contribution, or zero when no band covers the bin. Each harmonic's amplitude is computed once, on first
use, and cached, because every bin of a band shares the same amplitude. The pitch refinement error function
and the voiced/unvoiced decision both read the spectrum through this type.

| Field | Type | Meaning |
|---|---|---|
| `frame` | `&RefinementFrame`, borrowed | The analysed frame (its window spectrum, read through `sw_at(m)`); never modified here. |
| `omega0` | `f64`, radians per sample | The candidate fundamental whose harmonic bands are being tested. |
| `max_l` | `u32` | Highest harmonic index considered; bounds the search and the cache size. |
| `amplitudes` | `Vec<Option<Complex>>` | Cache of `A_l(omega0)` per harmonic, `None` until first needed; length `max_l + 1`. |

**Construction:** `SyntheticSpectrum::new(frame, omega0, max_l)` is crate-private and starts with an empty cache. **Reading:** `at(&mut self, m: i32) -> Complex` returns the spectrum at bin `m`: it walks harmonics `0..=max_l`, tests whether `m` lies in the band from `ceil(256 (l - 0.5) omega0 / 2pi)` up to but excluding `ceil(256 (l + 0.5) omega0 / 2pi)`, fills the cache with `harmonic_amplitude` on first use and scales it by the 16384-point window transform at the bin's offset from the harmonic; a bin in no band gives zero. **Invariants:** a cached value equals what `harmonic_amplitude` returns for that harmonic. **Concurrency and lifetime:** `at` needs `&mut self` for the lazy cache, so the value has one owner and lives only for the scoring of one candidate; it is never serialised. **See also:** the pitch refinement error function, which builds one per candidate, and the test tagged `SyntheticSpectrum`.

*(Reference: `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/pitch_refinement.rs`)*
