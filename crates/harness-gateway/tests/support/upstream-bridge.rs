//! Test helper child managed by ProcessManager. No model or GPU is loaded here.
//! The registry cannot attach to an arbitrary existing process; this loopback byte bridge
//! lets an isolated gateway use an existing llama-server without owning that server.
//! Compile with rustc; --bridge-target and --port are the only meaningful launch flags.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn relay(mut client: TcpStream, target: u16) -> std::io::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(300)))?;
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") {
        client.read_exact(&mut byte)?;
        request.push(byte[0]);
        if request.len() > 65536 {
            return Err(std::io::Error::other("headers too large"));
        }
    }
    let header = std::str::from_utf8(&request).map_err(std::io::Error::other)?;
    let length = header
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    let mut upstream = TcpStream::connect(("127.0.0.1", target))?;
    upstream.set_read_timeout(Some(Duration::from_secs(300)))?;
    let mut headers = header
        .lines()
        .filter(|l| !l.to_ascii_lowercase().starts_with("connection:") && !l.is_empty())
        .collect::<Vec<_>>()
        .join("\r\n");
    headers.push_str("\r\nConnection: close\r\n\r\n");
    upstream.write_all(headers.as_bytes())?;
    std::io::copy(&mut (&mut client).take(length as u64), &mut upstream)?;
    std::io::copy(&mut upstream, &mut client)?;
    Ok(())
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "--version") {
        println!("validation bridge --host --port --alias -m --ctx-size --parallel");
        return Ok(());
    }
    let get = |key: &str| {
        args.windows(2)
            .find(|w| w[0] == key)
            .and_then(|w| w[1].parse::<u16>().ok())
    };
    let target = get("--bridge-target").expect("--bridge-target required");
    let port = get("--port").expect("--port required");
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| {
        // ProcessManager classifies this diagnostic on every platform.
        if e.kind() == std::io::ErrorKind::AddrInUse {
            eprintln!("could not bind test bridge, port: {port}: {e}");
        }
        e
    })?;
    for client in listener.incoming() {
        let client = client?;
        std::thread::spawn(move || {
            if let Err(e) = relay(client, target) {
                eprintln!("bridge: {e}");
            }
        });
    }
    Ok(())
}
