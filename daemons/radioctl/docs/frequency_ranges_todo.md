# Frequency ranges still to confirm

`[frequency]` in a rig file (min_hz, max_hz) is enforced for every `SetFrequency` in `RigHandle::submit`.
A model with no `[frequency]` block accepts 0 Hz to 50 GHz. Ranges from Hamlib's receive lists are the
hull of its ranges (unverified). To do:

- Flex (all 12 files): 1 kHz to 500 MHz is a generous placeholder; take each model's and a transverter slice's real range from FlexRadio's documentation.
- IC-905: Hamlib hull 144 MHz to 10.5 GHz; check against Icom's CI-V reference (not in /storage/manuals).
- Every other CI-V, Kenwood, Yaesu, Elecraft and vendor model has no range yet (default 0 to 50 GHz): fill them in from their manuals.
