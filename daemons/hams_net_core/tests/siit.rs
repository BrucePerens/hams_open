#![allow(clippy::unwrap_used, clippy::expect_used)]
//! SIIT parity with `ipv6_siit/siit_map.py` through the shared vectors.
use hams_net_core::siit::Block;
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors/siit.json")).unwrap()
}

// Tests [@ANCHOR: siit_embed]
// Tests [@ANCHOR: siit_extract]
#[test]
fn embed_and_extract_match_the_python_vectors() {
    for case in vectors()["roundtrip"].as_array().unwrap() {
        let block = Block::parse(case["block"].as_str().unwrap()).unwrap();
        let domain = case["domain"].as_u64().unwrap() as u16;
        let v4 = case["ipv4"].as_str().unwrap().parse().unwrap();
        let v6 = block.embed(domain, v4);
        assert_eq!(v6.to_string(), case["ipv6"].as_str().unwrap());
        assert_eq!(block.extract(v6).unwrap(), (domain, v4));
    }
}

// Tests [@ANCHOR: siit_block_parse]
// Tests [@ANCHOR: siit_extract]
#[test]
fn refusals_match_the_python_vectors() {
    for bad in vectors()["bad_blocks"].as_array().unwrap() {
        assert!(Block::parse(bad.as_str().unwrap()).is_err(), "{bad}");
    }
    let v = vectors();
    let block = Block::parse(v["block"].as_str().unwrap()).unwrap();
    for bad in v["bad_extract"].as_array().unwrap() {
        assert!(
            block
                .extract(bad.as_str().unwrap().parse().unwrap())
                .is_err(),
            "{bad}"
        );
    }
}
