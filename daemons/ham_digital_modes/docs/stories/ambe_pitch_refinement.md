# Story: AMBE Encoder Pitch Refinement Synthetic Spectrum (ham_digital_modes)

The float AMBE (TIA-102.BABA) encoder refines a half-sample-accuracy pitch estimate to quarter-sample
accuracy by trying candidate fundamental frequencies and scoring each against the real frame spectrum.
The scoring needs the synthetic spectrum `S_w(m, omega0)` (Eq. 25): for a DFT bin `m`, it finds which
harmonic band of the candidate `omega0` the bin falls into (Eq. 26-27) and returns that harmonic's estimated
contribution, or zero when no band covers the bin. Each harmonic's amplitude is computed once, on first
use, and cached, because every bin of a band shares the same amplitude. The pitch refinement error function
and the voiced/unvoiced decision both read the spectrum through this type.
*(Reference: `daemons/ham_digital_modes/src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: ham_digital_modes:SyntheticSpectrum]`)*
