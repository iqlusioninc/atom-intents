use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Priority level for packets
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriorityLevel {
    /// Our own solver's packets - highest priority
    Own = 3,
    /// Paid relay requests
    Paid = 2,
    /// Altruistic relay
    Altruistic = 1,
}

/// A packet pending relay with priority
#[derive(Clone, Debug)]
pub struct PrioritizedPacket {
    pub packet_id: String,
    pub source_chain: String,
    pub dest_chain: String,
    pub channel: String,
    pub sequence: u64,
    pub priority_level: PriorityLevel,
    pub solver_exposure: u128,
    pub timeout_timestamp: u64,
    pub added_at: u64,
}

impl PrioritizedPacket {
    /// Calculate effective priority score
    pub fn priority_score(&self, current_time: u64) -> u64 {
        let base = (self.priority_level as u64) * 1_000_000_000;

        // Higher exposure = higher priority within level
        let exposure_factor = (self.solver_exposure / 1_000_000) as u64;

        // Closer to timeout = higher urgency
        let time_remaining = self.timeout_timestamp.saturating_sub(current_time);
        let urgency = 1_000_000u64.saturating_sub(time_remaining.min(1_000_000));

        base + exposure_factor * 1000 + urgency
    }
}

impl PartialEq for PrioritizedPacket {
    fn eq(&self, other: &Self) -> bool {
        self.packet_id == other.packet_id
    }
}

impl Eq for PrioritizedPacket {}

/// Wrapper for heap ordering
struct HeapEntry {
    packet: PrioritizedPacket,
    score: u64,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score.cmp(&other.score)
    }
}

/// Priority queue for packet relay
pub struct PriorityQueue {
    heap: BinaryHeap<HeapEntry>,
    current_time_fn: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl PriorityQueue {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            current_time_fn: Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            }),
        }
    }

    pub fn with_time_fn(time_fn: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self {
            heap: BinaryHeap::new(),
            current_time_fn: Box::new(time_fn),
        }
    }

    pub fn push(&mut self, packet: PrioritizedPacket) {
        let current_time = (self.current_time_fn)();
        let score = packet.priority_score(current_time);
        self.heap.push(HeapEntry { packet, score });
    }

    pub fn pop(&mut self) -> Option<PrioritizedPacket> {
        self.heap.pop().map(|e| e.packet)
    }

    pub fn peek(&self) -> Option<&PrioritizedPacket> {
        self.heap.peek().map(|e| &e.packet)
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Re-prioritize all packets (call periodically as time passes)
    pub fn refresh_priorities(&mut self) {
        let current_time = (self.current_time_fn)();
        let packets: Vec<_> = std::mem::take(&mut self.heap)
            .into_iter()
            .map(|e| e.packet)
            .collect();

        for packet in packets {
            let score = packet.priority_score(current_time);
            self.heap.push(HeapEntry { packet, score });
        }
    }
}

impl Default for PriorityQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        let mut queue = PriorityQueue::with_time_fn(|| 1000);

        // Add own packet
        queue.push(PrioritizedPacket {
            packet_id: "own-1".to_string(),
            source_chain: "hub".to_string(),
            dest_chain: "noble".to_string(),
            channel: "channel-0".to_string(),
            sequence: 1,
            priority_level: PriorityLevel::Own,
            solver_exposure: 100_000_000,
            timeout_timestamp: 2000,
            added_at: 1000,
        });

        // Add paid packet
        queue.push(PrioritizedPacket {
            packet_id: "paid-1".to_string(),
            source_chain: "hub".to_string(),
            dest_chain: "osmosis".to_string(),
            channel: "channel-1".to_string(),
            sequence: 2,
            priority_level: PriorityLevel::Paid,
            solver_exposure: 0,
            timeout_timestamp: 2000,
            added_at: 1000,
        });

        // Own should come first
        assert_eq!(queue.pop().unwrap().packet_id, "own-1");
        assert_eq!(queue.pop().unwrap().packet_id, "paid-1");
    }
}
