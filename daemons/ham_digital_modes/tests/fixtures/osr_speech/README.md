# `osr_speech` test fixtures

Real recorded speech from the [Open Speech Repository](http://www.voiptroubleshooter.com/open_speech/)
(OSR), a corpus published specifically for VoIP/codec/packet-loss-concealment testing. Used here to
exercise this crate's own AMBE (`src/ambe/`) and D-STAR AMBE (`src/ambe_dstar/`) codecs against real
human speech for the first time -- every test and live-chip-validation harness in this crate to date
has used only synthetic tones (pure sine waves or synthetic harmonic signals), never real recorded
voice, which is a real, previously-unaddressed gap in test coverage this fixture closes.

## Provenance and license

Four files from OSR's "American English" set (Harvard sentences -- phonetically balanced, the
standard reference sentence set for speech-intelligibility testing), two female (`OSR_us_000_0010_8k`,
`OSR_us_000_0011_8k`) and two male (`OSR_us_000_0030_8k`, `OSR_us_000_0031_8k`) voices, 8kHz 16-bit
mono PCM WAV -- already at this codec's own native sample rate, no resampling needed.

OSR's own stated conditions of use: "The material on this site is freely available for use in VoIP
testing, research, development, marketing and any other reasonable application. The material may be
copied, downloaded, broadcast, modified, incorporated into web sites or test equipment," with the
requirement that the source be identified as "Open Speech Repository" -- satisfied by this README and
the citation in `tests/ambe_real_speech_round_trip.rs`'s own module doc comment. Unlike this crate's
`codec2_3200`/`codec2_1600` fixtures (which keep real donated speech *out* of git even though cleared,
on the reasoning that hams_open is a public repository and redistribution wasn't the original donors'
own explicit grant), OSR's own terms explicitly authorize exactly this use, so the real WAV files are
committed here directly rather than gitignored.

Other candidate corpora considered and not used, per a licensing/fit comparison done before choosing
OSR: the **NISQA Corpus** (real-world-degradation speech with MOS quality scores) has non-commercial-
only restrictions on parts of its test sets, a real concern for a commercial-adjacent project; the
**OpenACE Benchmark** aggregates from ITU-T P.501/ETSI TS 103-281/VCTK, and the underlying ITU-T/ETSI
audio's own redistribution terms are not addressed anywhere in its own paper or repository (standards-
body reference material is typically restricted/paywalled) -- both would need real legal diligence OSR
simply doesn't require, given its own explicit, permissive, and directly on-point license.

## Regenerating / extending

Download more files directly from `http://www.voiptroubleshooter.com/open_speech/<language>/`
(American/British English, Mandarin, French, and Hindi sets are all available under the same terms) --
no special tooling needed, they're plain static file downloads.
