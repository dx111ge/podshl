//! The one HTTP client, and the one way a body is read.
//!
//! Every module that talked to the network built its own `reqwest::Client`,
//! and most of them built it with `Client::new()` — no connect timeout, no
//! total timeout, and a body read whole into memory whatever its size. `a2a`
//! learnt the timeout lesson the hard way (a host that accepts and never
//! answers froze the window for good); the other seven call sites had not
//! learnt it yet, and an operator or a model provider answering with a body of
//! any size could have filled the client's memory before it parsed a byte.
//!
//! One client, built once: five seconds to connect and thirty in total unless
//! a call says otherwise, which only the model calls do. Bodies are read in
//! chunks against a cap and refused past it, so the answer to "how large may
//! a response be" is written here rather than being whatever the peer sends.

use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);

/// What an ordinary JSON answer may weigh. A card, a remedy, a receipt, a
/// model's completion — none of them is a megabyte, and four is generous.
pub const MAX_BODY: usize = 4 * 1024 * 1024;
/// The catalogue is the one document fetched whole by design, and it grows
/// with the number of published projects.
pub const MAX_INDEX_BODY: usize = 64 * 1024 * 1024;

pub fn client() -> &'static reqwest::Client {
    static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TOTAL_TIMEOUT)
            // No idle connection is kept. A diagnosis makes a handful of
            // requests, each to a different host, so there is nothing for a
            // pool to reuse — and a pooled connection outlives the Tokio
            // runtime that opened it, which in the suite means one test hands
            // the next a socket whose reactor is gone and the request fails
            // as "error sending request" for no reason a reader could find.
            .pool_max_idle_per_host(0)
            .build()
            // The builder fails only when the TLS backend cannot be set up,
            // which is a build defect rather than a runtime condition — and a
            // client with no bounds is not a fallback worth having.
            .expect("the HTTP client could not be built")
    })
}

/// The body, read in chunks, refused the moment it passes `cap`. A declared
/// length past the cap is refused before a byte is read; an undeclared one
/// is refused as soon as it is seen to be too much.
pub async fn body_capped(resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, String> {
    if let Some(n) = resp.content_length() {
        if n > cap as u64 {
            return Err(m!("answer_too_large", max = cap));
        }
    }
    let mut resp = resp;
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if buf.len() + chunk.len() > cap {
            return Err(m!("answer_too_large", max = cap));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// The body as JSON, bounded first. `serde_json` parses from the bytes it is
/// given, so nothing larger than the cap ever reaches it.
pub async fn json_capped(resp: reqwest::Response, cap: usize) -> Result<Value, String> {
    let bytes = body_capped(resp, cap).await?;
    serde_json::from_slice(&bytes).map_err(|e| m!("answer_unreadable", e = e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Serve one HTTP answer of `len` bytes on a port of its own, without
    /// declaring a length, so the cap has to be enforced on what arrives
    /// rather than on what was promised.
    fn serve_once(len: usize, declare: bool) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("no port");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut sink = [0u8; 4096];
                let _ = s.read(&mut sink);
                let body = vec![b'x'; len];
                let head = if declare {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n")
                } else {
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_string()
                };
                let _ = s.write_all(head.as_bytes());
                if declare {
                    let _ = s.write_all(&body);
                } else {
                    for piece in body.chunks(1024) {
                        let _ = s.write_all(format!("{:x}\r\n", piece.len()).as_bytes());
                        let _ = s.write_all(piece);
                        let _ = s.write_all(b"\r\n");
                    }
                    let _ = s.write_all(b"0\r\n\r\n");
                }
                let _ = s.flush();
            }
        });
        format!("http://127.0.0.1:{port}/")
    }

    /// HT1: a body past the cap is refused, whether its length was declared
    /// or not, and one under it is read whole.
    #[tokio::test]
    async fn a_body_past_the_cap_is_refused_declared_or_not() {
        for declare in [true, false] {
            let url = serve_once(10_000, declare);
            let resp = client().get(&url).send().await.expect("no answer");
            let e = body_capped(resp, 8_000).await.expect_err("an oversized body was read whole");
            assert!(crate::msg::is("answer_too_large", &e), "{e}");

            let url = serve_once(3_000, declare);
            let resp = client().get(&url).send().await.expect("no answer");
            let got = body_capped(resp, 8_000).await.expect("a body under the cap was refused");
            assert_eq!(got.len(), 3_000);
        }
    }

    /// The one client carries the bounds every call site used to lack.
    #[test]
    fn the_shared_client_is_one_client() {
        let a = client() as *const reqwest::Client;
        let b = client() as *const reqwest::Client;
        assert_eq!(a, b, "two clients were built");
        assert!(CONNECT_TIMEOUT < TOTAL_TIMEOUT);
    }
}
