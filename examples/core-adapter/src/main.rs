use ruflo_core::{embed, route, EmbedRequest, RouteRequest};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let vector = embed(EmbedRequest {
        text: "route a Rust-native Ruflo integration".into(),
        dimensions: Some(16),
    })?;
    let decision = route(RouteRequest {
        task: "implement a native integration".into(),
        candidates: vec!["coder".into(), "reviewer".into()],
    })?;

    println!(
        "provider={} dimensions={} route={} score={}",
        vector.provider, vector.dimensions, decision.agent, decision.score
    );
    Ok(())
}
