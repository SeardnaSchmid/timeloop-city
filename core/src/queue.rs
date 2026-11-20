//! Event queue ordered by `(time, seq)`.
//!
//! `seq` is a running number assigned on push, so events in the same minute
//! always come out in the order they were scheduled, never in heap order.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::hash::StateHasher;
use crate::world::Minute;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Global: a new game day begins.
    DayStart,
    /// An agent finishes (or interrupts) its current action and decides again.
    Wake { agent: u32 },
}

impl Event {
    fn hash_into(&self, h: &mut StateHasher) {
        match *self {
            Event::DayStart => h.write_bytes(&[0]),
            Event::Wake { agent } => {
                h.write_bytes(&[1]);
                h.write_u32(agent);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scheduled {
    pub at: Minute,
    pub seq: u64,
    pub event: Event,
}

impl Ord for Scheduled {
    /// Reversed, so `BinaryHeap` (a max-heap) pops the earliest `(at, seq)` first.
    fn cmp(&self, other: &Self) -> Ordering {
        (other.at, other.seq).cmp(&(self.at, self.seq))
    }
}

impl PartialOrd for Scheduled {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Default)]
pub struct EventQueue {
    heap: BinaryHeap<Scheduled>,
    next_seq: u64,
    /// Events popped so far; a work counter, not part of the state hash.
    popped: u64,
}

impl EventQueue {
    pub fn push(&mut self, at: Minute, event: Event) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.heap.push(Scheduled { at, seq, event });
    }

    /// Pops the earliest event if it is due at or before `until`.
    pub fn pop_due(&mut self, until: Minute) -> Option<Scheduled> {
        if self.heap.peek()?.at <= until {
            self.popped += 1;
            self.heap.pop()
        } else {
            None
        }
    }

    /// Events popped since the queue was created.
    pub fn popped(&self) -> u64 {
        self.popped
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Hashes pending events in pop order, independent of heap layout.
    pub fn hash_into(&self, h: &mut StateHasher) {
        h.write_u64(self.next_seq);
        let mut pending: Vec<&Scheduled> = self.heap.iter().collect();
        pending.sort_by_key(|s| (s.at, s.seq));
        h.write_u64(pending.len() as u64);
        for s in pending {
            h.write_u64(s.at);
            h.write_u64(s.seq);
            s.event.hash_into(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(q: &mut EventQueue) -> Vec<(Minute, Event)> {
        std::iter::from_fn(|| q.pop_due(Minute::MAX))
            .map(|s| (s.at, s.event))
            .collect()
    }

    #[test]
    fn earliest_first() {
        let mut q = EventQueue::default();
        q.push(30, Event::Wake { agent: 1 });
        q.push(10, Event::Wake { agent: 2 });
        q.push(20, Event::Wake { agent: 3 });
        let order: Vec<_> = drain(&mut q).iter().map(|e| e.0).collect();
        assert_eq!(order, [10, 20, 30]);
    }

    #[test]
    fn same_minute_keeps_push_order() {
        let mut q = EventQueue::default();
        for agent in [5, 1, 9, 3, 7, 0, 2] {
            q.push(100, Event::Wake { agent });
        }
        q.push(100, Event::DayStart);
        let events: Vec<_> = drain(&mut q).into_iter().map(|e| e.1).collect();
        let expected: Vec<_> = [5, 1, 9, 3, 7, 0, 2]
            .into_iter()
            .map(|agent| Event::Wake { agent })
            .chain([Event::DayStart])
            .collect();
        assert_eq!(events, expected);
    }

    #[test]
    fn pop_due_respects_until() {
        let mut q = EventQueue::default();
        q.push(50, Event::DayStart);
        assert!(q.pop_due(49).is_none());
        assert!(q.pop_due(50).is_some());
        assert!(q.is_empty());
    }
}
