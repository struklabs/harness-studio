fn main() {
    println!(
        "{}",
        harness_api::openapi_document().to_pretty_json().unwrap()
    );
}
