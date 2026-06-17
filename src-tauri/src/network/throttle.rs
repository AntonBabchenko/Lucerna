//! Per-host outbound-request throttling: a token-bucket rate limiter with a
//! two-class priority queue, plus 429 host-freeze. Lives inside `network::`
//! so every chokepoint request is paced transparently. No reqwest here.

use std::sync::Arc;
use tokio::time::{Duration, Instant};

/// A token bucket for one host. `rate` tokens accrue per second up to `burst`.
pub struct HostGate {
    inner: std::sync::Mutex<Inner>,
}

struct Inner {
    tokens: f64,
    last_refill: Instant,
    rate: f64,
    burst: f64,
}

impl HostGate {
    /// Construct a gate. Public so tests build one directly (no global registry).
    pub fn new(rate: f64, burst: f64, now: Instant) -> Self {
        HostGate {
            inner: std::sync::Mutex::new(Inner { tokens: burst, last_refill: now, rate, burst }),
        }
    }

    fn refill(inner: &mut Inner, now: Instant) {
        let elapsed = now.saturating_duration_since(inner.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            inner.tokens = (inner.tokens + elapsed * inner.rate).min(inner.burst);
            inner.last_refill = now;
        }
    }

    /// Block until a token is available, then consume it.
    pub async fn acquire(&self) {
        loop {
            let wait = {
                let mut inner = self.inner.lock().unwrap();
                Self::refill(&mut inner, Instant::now());
                if inner.tokens >= 1.0 {
                    inner.tokens -= 1.0;
                    return;
                }
                let deficit = 1.0 - inner.tokens;
                Duration::from_secs_f64(deficit / inner.rate)
            };
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn bucket_paces_after_burst_is_drained() {
        let gate = Arc::new(HostGate::new(5.0, 2.0, Instant::now())); // 5/s, burst 2
        gate.acquire().await; // burst token 1 (immediate)
        gate.acquire().await; // burst token 2 (immediate)
        // 3rd must wait ~0.2s (1 token / 5 per sec).
        let g2 = gate.clone();
        let h = tokio::spawn(async move { g2.acquire().await });
        tokio::time::advance(Duration::from_millis(150)).await;
        assert!(!h.is_finished(), "3rd acquire should still be waiting at 150ms");
        tokio::time::advance(Duration::from_millis(100)).await;
        h.await.unwrap();
    }
}
