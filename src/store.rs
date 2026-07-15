use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Store {
    map: Mutex<HashMap<String, Entry>>,
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
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: String) -> Option<String> {
        
        let mut map = self.map.lock().unwrap();

        let expired = match map.get(&key) {
            None => return None,                         // no such key
            Some(entry) => match entry.expires_at {
                None => false,                           // no ttl → immortal
                Some(deadline) => Instant::now() > deadline,
            },
        };

        if expired {
            map.remove(&key);
            return None;                                 // expired = never existed
        }
        Some(map.get(&key).unwrap().value.clone())

    }

    pub fn set(&self, key: String, value: String, ttl: Option<u64>) {

        let mut map = self.map.lock().unwrap();
        map.insert(key, Entry::new(value, ttl));
    }
    pub fn delete(&self, key: String) -> bool {
        let mut map = self.map.lock().unwrap();
        map.remove(&key).is_some()
    }
}
