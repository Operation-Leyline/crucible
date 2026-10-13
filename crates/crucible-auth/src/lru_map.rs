use std::collections::{BTreeMap, HashMap};

/// A capacity-bounded map that evicts the least recently used entry, keyed by
/// digest. Hand-rolled to avoid a dependency: recency is a monotonic tick, and
/// the ordered index finds the oldest in O(log n), which is negligible beside a
/// store read.
pub(crate) struct LruMap<V> {
    capacity: usize,
    tick: u64,
    entries: HashMap<String, (V, u64)>,
    order: BTreeMap<u64, String>,
}

impl<V> LruMap<V> {
    /// A capacity of zero stores nothing.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            tick: 0,
            entries: HashMap::new(),
            order: BTreeMap::new(),
        }
    }

    /// Reads an entry and marks it most recently used.
    pub fn get(&mut self, key: &str) -> Option<&V> {
        self.tick += 1;
        let tick = self.tick;
        let (value, used) = self.entries.get_mut(key)?;
        let key = self.order.remove(used).expect("LRU order matches entries");
        *used = tick;
        self.order.insert(tick, key);
        Some(value)
    }

    /// Reads an entry without changing its recency.
    pub fn peek(&self, key: &str) -> Option<&V> {
        self.entries.get(key).map(|(value, _)| value)
    }

    pub fn insert(&mut self, key: &str, value: V) {
        if self.capacity == 0 {
            return;
        }
        self.remove(key);
        while self.entries.len() >= self.capacity {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.tick += 1;
        self.order.insert(self.tick, key.to_owned());
        self.entries.insert(key.to_owned(), (value, self.tick));
    }

    pub fn remove(&mut self, key: &str) -> Option<V> {
        let (value, used) = self.entries.remove(key)?;
        self.order.remove(&used);
        Some(value)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::LruMap;

    #[test]
    fn evicts_least_recently_used() {
        let mut map = LruMap::new(2);
        map.insert("a", 1);
        map.insert("b", 2);
        assert_eq!(map.get("a"), Some(&1));
        map.insert("c", 3);
        assert_eq!(map.peek("b"), None);
        assert_eq!(map.peek("a"), Some(&1));
        assert_eq!(map.peek("c"), Some(&3));
        map.insert("a", 4);
        assert_eq!(map.len(), 2);
        assert_eq!(map.remove("a"), Some(4));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn zero_capacity_stores_nothing() {
        let mut map = LruMap::new(0);
        map.insert("a", 1);
        assert_eq!(map.peek("a"), None);
    }
}
