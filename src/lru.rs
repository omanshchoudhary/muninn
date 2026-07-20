use std::collections::HashMap;

#[derive(Default)]
struct Node {
    key: String,
    next: Option<usize>,
    prev: Option<usize>,
}

impl Node {
    fn new(key: String) -> Self {
        Self {
            key,
            next: None,
            prev: None,
        }
    }
}

pub struct Lru {
    arena: Vec<Node>,
    head: Option<usize>, // represent MRU
    tail: Option<usize>, // represent LRU
    mapping: HashMap<String, usize>,
    slots: Vec<usize>,
}

impl Lru {
    fn new() -> Self {
        Self {
            arena: Vec::new(),
            head: None,
            tail: None,
            mapping: HashMap::new(),
            slots: Vec::new(),
        }
    }

    // Insert a completely new key as the MRU.
    pub fn insert(&mut self, key: String) {
        // get an index
        let index = self.alloc_slot();

        self.arena[index].key = key.clone();

        // make it head
        self.insert_head(index);
        self.mapping.insert(key, index);
    }

    // Called when a key is accessed and make it MRU
    pub fn touch(&mut self, key: &str) {
        if let Some(&index) = self.mapping.get(key) {
            self.move_to_head(index);
        }
    }

    // Remove the LRU and return its key.
    pub fn evict(&mut self) -> Option<String> {
        let index = self.tail?; // empty list → nothing to evict

        // we already have the index — no need to look it back up in the map
        let key = self.arena[index].key.clone();

        self.unlink(index);
        self.mapping.remove(&key); // key leaves the list → leaves every structure
        self.free_slot(index);
        Some(key)
    }

    // Returns the new index
    fn alloc_slot(&mut self) -> usize {
        let index = if let Some(i) = self.slots.pop() {
            i
        } else {
            self.arena.push(Node::default());
            self.arena.len() - 1
        
        };
        index
    }

    fn free_slot(&mut self, index: usize) {
        self.slots.push(index);
    }

    // Make the new node as head of list
    fn insert_head(&mut self, index: usize) {
        
        // this lone node is both head and tail, no neighbours to wire
        if self.head.is_none() {
            self.arena[index].prev = None;
            self.arena[index].next = None;
            self.head = Some(index);
            self.tail = Some(index);
            return;
        }

        let old_head = self.head;
        let node = &mut self.arena[index];
        node.prev = None;
        node.next = old_head;

        if let Some(h) = old_head {
            self.arena[h].prev = Some(index);
        }
        self.head = Some(index);
    }

    fn unlink(&mut self, index: usize) {
        // copy the neighbour indices out first, so we're not holding a borrow
        // on arena[index] while we reach into other slots to rewire them
        let prev = self.arena[index].prev;
        let next = self.arena[index].next;

        // repair the forward link that pointed INTO this node
        match prev {
            Some(p) => self.arena[p].next = next, // a real node sat before us
            None => self.head = next,             // we were the head
        }

        // repair the backward link that pointed INTO this node
        match next {
            Some(n) => self.arena[n].prev = prev, // a real node sat after us
            None => self.tail = prev,             // we were the tail
        }
    }

    fn move_to_head(&mut self, index: usize) {
        if self.head == Some(index) {
            return;
        }

        self.unlink(index);
        self.insert_head(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_in_insertion_order() {
        let mut lru = Lru::new();
        lru.insert("a".into());
        lru.insert("b".into());
        lru.insert("c".into());
        // oldest first
        assert_eq!(lru.evict(), Some("a".into()));
        assert_eq!(lru.evict(), Some("b".into()));
        assert_eq!(lru.evict(), Some("c".into()));
        assert_eq!(lru.evict(), None); // empty
    }

    #[test]
    fn touch_promotes_to_most_recent() {
        let mut lru = Lru::new();
        lru.insert("a".into());
        lru.insert("b".into());
        lru.insert("c".into());
        lru.touch("a"); // a is now freshest, so b is the oldest
        assert_eq!(lru.evict(), Some("b".into()));
    }

    #[test]
    fn freed_slots_are_reused() {
        let mut lru = Lru::new();
        lru.insert("a".into());
        lru.evict(); // frees slot 0
        lru.insert("b".into()); // should reuse slot 0, not grow the arena
        assert_eq!(lru.arena.len(), 1);
    }
}
