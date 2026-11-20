use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::models::Profile;

/// Measure TCP connect latency in milliseconds, or -1 if unreachable/timed out.
pub async fn tcp_ping(host: &str, port: u16, timeout_ms: u64) -> i64 {
    let start = Instant::now();
    let addr = format!("{host}:{port}");
    match timeout(Duration::from_millis(timeout_ms), TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => start.elapsed().as_millis() as i64,
        _ => -1,
    }
}

/// Concurrently TCP-ping a slice of profiles and return `(profile_id, latency_ms)`.
pub async fn ping_all(profiles: &[Profile], concurrency: usize, timeout_ms: u64) -> Vec<(String, i64)> {
    let sem = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut set = JoinSet::new();

    for p in profiles {
        let permit_sem = Arc::clone(&sem);
        let id = p.id.clone();
        let host = p.server.clone();
        let port = p.port;
        set.spawn(async move {
            let _permit = permit_sem.acquire_owned().await;
            let ms = tcp_ping(&host, port, timeout_ms).await;
            (id, ms)
        });
    }

    let mut results = Vec::with_capacity(profiles.len());
    while let Some(res) = set.join_next().await {
        if let Ok(pair) = res {
            results.push(pair);
        }
    }
    results
}
