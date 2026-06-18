//! Per-host outbound-request throttling: a token-bucket rate limiter with a
//! two-class priority queue, plus 429 host-freeze. Lives inside `network::`
//! so every chokepoint request is paced transparently. No reqwest here.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::Notify;
use tokio::time::{Duration, Instant};

/// Request priority. User-clicked work is `Interactive` and jumps the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Interactive,
    Background,
}

tokio::task_local! {
    /// Ambient priority for the current task. Unset -> `Background`.
    static PRIORITY: Priority;
}

/// Run `fut` with `Interactive` priority for all chokepoint requests it makes
/// on the same task (does NOT cross `tokio::spawn`).
pub async fn with_interactive<F: std::future::Future>(fut: F) -> F::Output {
    PRIORITY.scope(Priority::Interactive, fut).await
}

/// The current task's priority, or `Background` when unset.
pub fn current_priority() -> Priority {
    PRIORITY.try_with(|p| *p).unwrap_or(Priority::Background)
}

struct Inner {
    tokens: f64,
    last_refill: Instant,
    rate: f64,
    burst: f64,
    frozen_until: Option<Instant>,
    interactive: VecDeque<Arc<Notify>>,
    background: VecDeque<Arc<Notify>>,
}

/// A token bucket for one host. `rate` tokens accrue per second up to `burst`.
pub struct HostGate {
    inner: std::sync::Mutex<Inner>,
}

impl HostGate {
    /// Construct a gate. Public so tests build one directly (no global registry).
    pub fn new(rate: f64, burst: f64, now: Instant) -> Self {
        HostGate {
            inner: std::sync::Mutex::new(Inner {
                tokens: burst,
                last_refill: now,
                rate,
                burst,
                frozen_until: None,
                interactive: VecDeque::new(),
                background: VecDeque::new(),
            }),
        }
    }

    /// Freeze the host (no grants) for `dur` from now — set on a 429.
    pub fn freeze(&self, dur: Duration) {
        let mut inner = self.inner.lock().unwrap();
        inner.frozen_until = Some(Instant::now() + dur);
    }

    fn refill(inner: &mut Inner, now: Instant) {
        let elapsed = now
            .saturating_duration_since(inner.last_refill)
            .as_secs_f64();
        if elapsed > 0.0 {
            inner.tokens = (inner.tokens + elapsed * inner.rate).min(inner.burst);
            inner.last_refill = now;
        }
    }

    /// Block until a token is available and this waiter is the front-runner for
    /// its class, then consume the token. `Interactive` waiters jump ahead of
    /// all `Background` waiters.
    pub async fn acquire(&self, priority: Priority) {
        let me = Arc::new(Notify::new());
        {
            let mut inner = self.inner.lock().unwrap();
            match priority {
                Priority::Interactive => inner.interactive.push_back(me.clone()),
                Priority::Background => inner.background.push_back(me.clone()),
            }
        }
        // RAII guard: removes `me` from its queue by identity on EVERY exit of
        // this future — a successful grant (return below) and, crucially, a
        // mid-wait cancellation (the future being dropped). On drop it also
        // wakes the new head, so the slot this waiter held is never stranded.
        let _guard = WaiterGuard {
            gate: self,
            me: me.clone(),
            priority,
        };
        loop {
            let wait = {
                let mut inner = self.inner.lock().unwrap();
                let now = Instant::now();
                Self::refill(&mut inner, now);

                let is_front = match priority {
                    Priority::Interactive => inner
                        .interactive
                        .front()
                        .is_some_and(|f| Arc::ptr_eq(f, &me)),
                    Priority::Background => {
                        inner.interactive.is_empty()
                            && inner
                                .background
                                .front()
                                .is_some_and(|f| Arc::ptr_eq(f, &me))
                    }
                };
                let frozen = inner.frozen_until.is_some_and(|t| now < t);

                if is_front && !frozen && inner.tokens >= 1.0 {
                    inner.tokens -= 1.0;
                    // Don't remove `me` or wake here: `_guard`'s Drop is the
                    // single removal+wake site for every exit (grant + cancel).
                    // Returning runs Drop, which removes `me` by identity and
                    // wakes the new head — same effect as the old pop_front.
                    return;
                }

                let by_freeze = inner
                    .frozen_until
                    .and_then(|t| t.checked_duration_since(now));
                let by_token = if inner.tokens >= 1.0 {
                    None
                } else {
                    Some(Duration::from_secs_f64((1.0 - inner.tokens) / inner.rate))
                };
                [by_freeze, by_token]
                    .into_iter()
                    .flatten()
                    .min()
                    .unwrap_or(Duration::from_millis(50))
            };
            // `Notify::notify_one` stores a permit if it races ahead of this
            // `notified()`, so a wake delivered between loop iterations is not
            // lost — the next `notified()` returns immediately.
            tokio::select! {
                _ = me.notified() => {}
                _ = tokio::time::sleep(wait) => {}
            }
        }
    }

    /// Wake the current front-runner (interactive head, else background head)
    /// so it re-checks. Called after a grant frees a slot.
    fn wake_next_front(inner: &Inner) {
        if let Some(f) = inner.interactive.front() {
            f.notify_one();
        } else if let Some(f) = inner.background.front() {
            f.notify_one();
        }
    }
}

/// Removes this waiter from its queue when the `acquire` future is dropped —
/// on cancellation OR after a grant. Keyed by `Arc::ptr_eq` so removal is
/// identity-based, not positional. On drop it also wakes the new head so a slot
/// freed by this waiter's departure is never stranded (the cancellation case
/// where a phantom interactive head would otherwise block all background work).
struct WaiterGuard<'a> {
    gate: &'a HostGate,
    me: Arc<Notify>,
    priority: Priority,
}

impl Drop for WaiterGuard<'_> {
    fn drop(&mut self) {
        let mut inner = self.gate.inner.lock().unwrap();
        let q = match self.priority {
            Priority::Interactive => &mut inner.interactive,
            Priority::Background => &mut inner.background,
        };
        if let Some(pos) = q.iter().position(|n| Arc::ptr_eq(n, &self.me)) {
            q.remove(pos);
        }
        // If we were the head others were blocked behind, wake the new front.
        HostGate::wake_next_front(&inner);
    }
}

// per-host registry + config

struct HostConfig {
    rate: f64,
    burst: f64,
}

/// Tokens/sec + burst per host. Modrinth's documented limit is 300/min = 5/s.
/// Unknown hosts (CDNs, Mojang, download mirrors) get an effectively-unlimited
/// bucket so file downloads are never throttled.
fn config_for(host: &str) -> HostConfig {
    match host {
        "api.modrinth.com" => HostConfig {
            rate: 5.0,
            burst: 20.0,
        },
        "api.curseforge.com" => HostConfig {
            rate: 5.0,
            burst: 20.0,
        },
        _ => HostConfig {
            rate: 1.0e9,
            burst: 1.0e9,
        },
    }
}

fn registry() -> &'static std::sync::Mutex<HashMap<String, Arc<HostGate>>> {
    static REG: OnceLock<std::sync::Mutex<HashMap<String, Arc<HostGate>>>> = OnceLock::new();
    REG.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Get (or lazily create) the gate for `host`.
pub fn gate_for(host: &str) -> Arc<HostGate> {
    let mut reg = registry().lock().unwrap();
    if let Some(g) = reg.get(host) {
        return g.clone();
    }
    let cfg = config_for(host);
    let g = Arc::new(HostGate::new(cfg.rate, cfg.burst, Instant::now()));
    reg.insert(host.to_string(), g.clone());
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn bucket_paces_after_burst_is_drained() {
        let gate = Arc::new(HostGate::new(5.0, 2.0, Instant::now())); // 5/s, burst 2
        gate.acquire(Priority::Background).await; // burst token 1 (immediate)
        gate.acquire(Priority::Background).await; // burst token 2 (immediate)
                                                  // 3rd must wait ~0.2s (1 token / 5 per sec).
        let g2 = gate.clone();
        let h = tokio::spawn(async move { g2.acquire(Priority::Background).await });
        tokio::time::advance(Duration::from_millis(150)).await;
        assert!(
            !h.is_finished(),
            "3rd acquire should still be waiting at 150ms"
        );
        tokio::time::advance(Duration::from_millis(100)).await;
        h.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn interactive_waiter_is_served_before_background() {
        let gate = Arc::new(HostGate::new(5.0, 1.0, Instant::now())); // 1 burst token
        gate.acquire(Priority::Background).await; // drains the single token

        let order = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
        let mk = |g: Arc<HostGate>,
                  p: Priority,
                  tag: &'static str,
                  order: Arc<std::sync::Mutex<Vec<&'static str>>>| {
            tokio::spawn(async move {
                g.acquire(p).await;
                order.lock().unwrap().push(tag);
            })
        };
        let b1 = mk(gate.clone(), Priority::Background, "b1", order.clone());
        let b2 = mk(gate.clone(), Priority::Background, "b2", order.clone());
        tokio::time::sleep(Duration::from_millis(1)).await; // let them enqueue
        let i1 = mk(gate.clone(), Priority::Interactive, "i1", order.clone());
        tokio::time::sleep(Duration::from_millis(1)).await;

        tokio::time::advance(Duration::from_secs(1)).await; // plenty of tokens
        i1.await.unwrap();
        b1.await.unwrap();
        b2.await.unwrap();
        assert_eq!(
            order.lock().unwrap()[0],
            "i1",
            "interactive must be served first"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn freeze_blocks_until_unfrozen() {
        let gate = Arc::new(HostGate::new(100.0, 100.0, Instant::now())); // plenty of tokens
        gate.freeze(Duration::from_secs(2));
        let g = gate.clone();
        let h = tokio::spawn(async move { g.acquire(Priority::Interactive).await });
        tokio::time::advance(Duration::from_millis(1500)).await;
        assert!(!h.is_finished(), "frozen host must hold even with tokens");
        tokio::time::advance(Duration::from_millis(600)).await;
        h.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_interactive_waiter_unblocks_background() {
        let gate = Arc::new(HostGate::new(5.0, 1.0, Instant::now())); // 1 burst token
        gate.acquire(Priority::Background).await; // drain the only token

        // An interactive waiter enqueues at the head, then is cancelled (dropped).
        let g = gate.clone();
        let interactive = tokio::spawn(async move { g.acquire(Priority::Interactive).await });
        tokio::time::sleep(Duration::from_millis(1)).await; // let it enqueue at head
        interactive.abort(); // drops the acquire future mid-wait → must remove the phantom
        let _ = interactive.await; // joins (cancelled)

        // A background waiter must now be able to proceed once tokens refill.
        // Without the drop guard, the phantom interactive head blocks it forever.
        let g2 = gate.clone();
        let bg = tokio::spawn(async move { g2.acquire(Priority::Background).await });
        tokio::time::advance(Duration::from_secs(1)).await; // refill tokens
        tokio::time::timeout(Duration::from_secs(5), bg)
            .await
            .expect("background acquire must not hang after the interactive waiter was cancelled")
            .unwrap();
    }

    #[tokio::test]
    async fn with_interactive_sets_ambient_priority() {
        assert_eq!(current_priority(), Priority::Background);
        with_interactive(async {
            assert_eq!(current_priority(), Priority::Interactive);
        })
        .await;
        assert_eq!(current_priority(), Priority::Background);
    }
}
