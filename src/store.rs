use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct Store {
    inner: Mutex<Inner>,
    max_memory: usize,
}

// Need both map and memory_used to be consistent that's why locked in the same room
struct Inner {
    map: HashMap<String, Entry>,
    memory_used: usize,
}

struct Entry {
    value: String,
    expires_at: Option<Instant>,
}
impl Entry {
    fn new(value: String, ttl_secs: Option<u64>) -> Self {
        Self {
            value,
            // convert the relative ttl into an absolute deadline, once, right now
            expires_at: ttl_secs.map(|secs| Instant::now() + Duration::from_secs(secs)),
        }
    }
}

impl Store {
    pub fn new(max_memory: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                map: HashMap::new(),
                memory_used: 0,
            }),
            max_memory,
        }
    }

    pub fn get(&self, key: String) -> Option<String> {
        let mut inner = self.inner.lock().unwrap();

        let expired = match inner.map.get(&key) {
            None => return None, // no such key
            Some(entry) => match entry.expires_at {
                None => false, // no ttl → immortal
                Some(deadline) => Instant::now() > deadline,
            },
        };

        if expired {
            if let Some(old) = inner.map.remove(&key) {
                inner.memory_used -= key.len() + old.value.len();
            }
            return None; // expired = never existed
        }
        Some(inner.map.get(&key).unwrap().value.clone())
    }

    pub fn set(&self, key: String, value: String, ttl: Option<u64>) {
        let key_len = key.len(); // grab lengths BEFORE key moves
        let new_size = key_len + value.len();

        let mut inner = self.inner.lock().unwrap(); // lock first and then everything 

        // returns the old value if one existed
        if let Some(old) = inner.map.insert(key, Entry::new(value, ttl)) {
            inner.memory_used -= key_len + old.value.len();
        }
        inner.memory_used += new_size;
    }

    pub fn delete(&self, key: String) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match inner.map.remove(&key) {
            Some(old) => {
                inner.memory_used -= key.len() + old.value.len();
                true
            }
            None => false,
        }
    }
}
