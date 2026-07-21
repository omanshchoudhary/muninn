use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::lru::Lru;

pub struct Store {
    inner: Mutex<Inner>,
    max_memory: usize,
}

// Need both map and memory_used to be consistent that's why locked in the same room
struct Inner {
    map: HashMap<String, Entry>,
    memory_used: usize,
    lru: Lru,
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
                lru: Lru::new(),
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
                inner.lru.remove(&key)
            }
            return None; // expired = never existed
        }
        inner.lru.touch(&key);
        Some(inner.map.get(&key).unwrap().value.clone())
    }

    pub fn set(&self, key: String, value: String, ttl: Option<u64>) -> Result<(), &'static str> {
        let key_len = key.len(); // grab lengths BEFORE key moves
        let new_size = key_len + value.len();

        let mut inner = self.inner.lock().unwrap(); // lock first and then everything 

        while self.max_memory > 0 && inner.memory_used + new_size > self.max_memory {
            match inner.lru.evict() {
                Some(victim) => {
                    if let Some(old) = inner.map.remove(&victim) {
                        inner.memory_used -= victim.len() + old.value.len();
                    }
                }
                None => return Err("OOM: entry larger than max-memory"), // nothing left to evict, still won't fit
            }
        }

        // insert into the map; the return tells us new-key vs overwrite
        match inner.map.insert(key.clone(), Entry::new(value, ttl)) {
            Some(old) => {
                // overwrite: key already lives in the lru, just promote it
                inner.memory_used -= key_len + old.value.len();
                inner.lru.touch(&key);
            }
            None => inner.lru.insert(key), // brand-new key, hand ownership over
        }
        inner.memory_used += new_size;
        Ok(())
    }

    pub fn delete(&self, key: String) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match inner.map.remove(&key) {
            Some(old) => {
                inner.memory_used -= key.len() + old.value.len();
                inner.lru.remove(&key);
                true
            }
            None => false,
        }
    }
}
