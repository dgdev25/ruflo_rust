use std::env;
use std::time::Instant;

use ruflo_core::{embed, EmbedRequest};

fn fingerprint(values: &[f64]) -> String {
    let hash = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    format!("{hash:016x}")
}

fn main() {
    let iterations = env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(30_usize);
    let request = EmbedRequest {
        text: "native addon parity benchmark fixture".into(),
        dimensions: Some(384),
    };
    let expected = embed(request.clone()).expect("fixed benchmark fixture is valid");
    let mut samples_ns = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        let answer = embed(request.clone()).expect("fixed benchmark fixture is valid");
        samples_ns.push(started.elapsed().as_nanos());
        assert_eq!(
            answer.vector, expected.vector,
            "core result changed during benchmark"
        );
    }
    println!(
        "{{\"provider\":\"{}\",\"dimensions\":{},\"fingerprint\":\"{}\",\"samples_ns\":{:?}}}",
        expected.provider,
        expected.dimensions,
        fingerprint(&expected.vector),
        samples_ns
    );
}
