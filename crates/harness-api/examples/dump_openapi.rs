fn main() {
    // The complete debug-build router/schema can exceed Windows' default main-thread stack.
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            println!(
                "{}",
                harness_api::openapi_document().to_pretty_json().unwrap()
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
