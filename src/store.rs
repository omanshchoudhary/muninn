use std::{collections::HashMap, sync::Mutex};

#[derive(Default)]
pub struct Store {
    map: Mutex<HashMap<String, String>>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: String) -> Option<String> {
       self.map.lock().unwrap().get(&key).cloned()
    }

    pub fn set(&self, key: String, value: String) -> Option<String> {
        let mut map = self.map.lock().unwrap();
        map.insert(key, value)
    }
    pub fn delete(&self, key: String) -> bool {
        let mut map = self.map.lock().unwrap();
        map.remove(&key).is_some()
    }
}
