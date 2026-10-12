//! The transport probe must distinguish route matching from global body rejection.
use harness_core::capabilities::verify::{probe_log_notes, probe_server, ProbeTarget};
use harness_core::serve_mode::ServeMode;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

async fn check(route_code: u16, missing_code: u16) -> Option<bool> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    let server = std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let path = first.split_whitespace().nth(1).unwrap();
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                headers.push_str(&line);
            }
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            reader.read_exact(&mut vec![0; length]).unwrap();
            let code = match path {
                "/v1/responses" => {
                    assert!(first.starts_with("POST "));
                    assert!(headers.to_lowercase().contains("content-encoding: gzip"));
                    route_code
                }
                "/__harness_missing_responses_probe" => missing_code,
                _ => 404,
            };
            write!(
                stream,
                "HTTP/1.1 {code} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        }
    });
    let report = probe_server(&ProbeTarget {
        base_url: format!("http://{address}"),
        api_key: None,
        router_mode: false,
        has_mmproj: false,
        serve_mode: ServeMode::Chat,
    })
    .await;
    stopped.store(true, Ordering::Relaxed);
    server.join().unwrap();
    assert!(probe_log_notes(&report, "tr").is_empty());
    report.features.get("responses_api").map(|v| v.ok)
}

#[tokio::test]
async fn route_rejects_gzip_without_entering_json_handler() {
    assert_eq!(check(400, 404).await, Some(true));
    assert_eq!(check(415, 404).await, Some(true));
}

#[tokio::test]
async fn missing_disabled_and_unauthorized_routes_are_not_verified() {
    assert_eq!(check(404, 404).await, Some(false));
    assert_eq!(check(501, 404).await, Some(false));
    assert_eq!(check(401, 404).await, None);
}

#[tokio::test]
async fn decoding_before_routing_is_inconclusive() {
    assert_eq!(check(400, 400).await, None);
    assert_eq!(check(415, 415).await, None);
}
