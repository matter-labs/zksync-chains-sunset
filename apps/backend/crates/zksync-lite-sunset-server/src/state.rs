use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;

use zksync_lite_sunset_core::world::World;

const READ_REQUEST_LIMIT: usize = 32;
const SPONSORED_CLAIM_WRITE_LIMIT: usize = 1;
const MIGRATION_WRITE_LIMIT: usize = 8;

#[derive(Clone)]
pub struct AppState {
    pub world: World,
    started_at: Instant,
    throttle: RequestThrottleState,
}

impl AppState {
    pub fn new(world: World) -> Self {
        Self {
            world,
            started_at: Instant::now(),
            throttle: RequestThrottleState::new(),
        }
    }

    pub fn uptime_seconds(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub(crate) fn try_acquire_read_request(&self) -> Option<InFlightRequestPermit> {
        self.throttle.read_requests.try_acquire()
    }

    pub(crate) fn try_acquire_sponsored_claim_write(&self) -> Option<InFlightRequestPermit> {
        self.throttle.sponsored_claim_writes.try_acquire()
    }

    pub(crate) fn try_acquire_migration_write(&self) -> Option<InFlightRequestPermit> {
        self.throttle.migration_writes.try_acquire()
    }
}

#[derive(Clone)]
struct RequestThrottleState {
    read_requests: Arc<ConcurrencyGate>,
    sponsored_claim_writes: Arc<ConcurrencyGate>,
    migration_writes: Arc<ConcurrencyGate>,
}

impl RequestThrottleState {
    fn new() -> Self {
        Self {
            read_requests: Arc::new(ConcurrencyGate::new(READ_REQUEST_LIMIT)),
            sponsored_claim_writes: Arc::new(ConcurrencyGate::new(SPONSORED_CLAIM_WRITE_LIMIT)),
            migration_writes: Arc::new(ConcurrencyGate::new(MIGRATION_WRITE_LIMIT)),
        }
    }
}

#[derive(Debug)]
struct ConcurrencyGate {
    max_in_flight: usize,
    in_flight: AtomicUsize,
}

impl ConcurrencyGate {
    const fn new(max_in_flight: usize) -> Self {
        Self {
            max_in_flight,
            in_flight: AtomicUsize::new(0),
        }
    }

    fn try_acquire(self: &Arc<Self>) -> Option<InFlightRequestPermit> {
        loop {
            let current = self.in_flight.load(Ordering::Acquire);
            if current >= self.max_in_flight {
                return None;
            }

            if self
                .in_flight
                .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(InFlightRequestPermit {
                    gate: Arc::clone(self),
                });
            }
        }
    }
}

pub(crate) struct InFlightRequestPermit {
    gate: Arc<ConcurrencyGate>,
}

impl Drop for InFlightRequestPermit {
    fn drop(&mut self) {
        self.gate.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::ConcurrencyGate;
    use std::sync::Arc;

    #[test]
    fn concurrency_gate_rejects_when_limit_is_reached() {
        let gate = Arc::new(ConcurrencyGate::new(1));

        let first = gate.try_acquire();
        assert!(first.is_some());
        assert!(gate.try_acquire().is_none());

        drop(first);

        assert!(gate.try_acquire().is_some());
    }
}
