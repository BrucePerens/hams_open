# Frequency ranges still to confirm

`[frequency]` in a rig file (min_hz, max_hz) is enforced for every `SetFrequency` in `RigHandle::submit`.
A model with no `[frequency]` block accepts 0 Hz to 50 GHz.

Done (range from the manual in /storage/manuals, `frequency.*` fact is manual-checked, test in
`crates/radioctl-protocols/tests/rig_files.rs`): FT-450, FT-450D, FT-710, FT-817, FT-857D, FT-891, FT-897D,
FT-991A, FTDX10, FTDX101D/MP, FTDX1200, FTDX3000, FTDX5000, FTDX9000 (and its Old model), IC-7300, IC-7610,
IC-705, IC-9100, K3, K3S, KX2, KX3, K4, TS-590S/SG, TS-2000 (upper limit is a margin; see the file's note).

Still to do (needs a manual that is not in /storage/manuals, or one that is silent):

- Flex (all 12 files): 1 kHz to 10.5 GHz is a placeholder (wide enough for a slice on a transverter band). No FlexRadio manual is on the box; take each model's and a transverter slice's real range from FlexRadio's documentation.
- IC-905: Hamlib hull 144 MHz to 10.5 GHz; Icom's CI-V reference (IC-905) is not in /storage/manuals.
- hobbypcb-rs-hfiq (3.5-30 MHz), codan-envoy and codan-ngt (1.6-30 MHz): Hamlib-derived; no manual on the box.
- Other Hamlib-derived ranges (adat-adt-200a, gomspace-gs100, lowe-hf-235, racal-ra3702, racal-ra6790-gm, rft-ekd-500, rs-eb200, rs-esmc, tentec-rx-331, tentec-rx-340, tentec-tt-565, tentec-tt-599): no manual on the box.
- TS-480 (the manual only says the RX band-pass filters span 500 kHz to 60 MHz), TS-890S, K2 (no coverage line found): manuals on the box are silent; no range set.
- Every other CI-V, Kenwood, Yaesu, Elecraft and vendor model has no range yet (default 0 to 50 GHz); a manual is needed first.
