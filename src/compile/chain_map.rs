use std::{borrow::Borrow, collections::HashMap, hash::Hash};

#[derive(Clone, Debug)]
pub struct ChainMap<'a, K, V> {
    map: HashMap<K, V>,
    parent: Option<&'a ChainMap<'a, K, V>>,
}

impl<'a, K: Eq + Hash, V> ChainMap<'a, K, V> {
    pub fn new(map: HashMap<K, V>) -> Self {
        Self { map, parent: None }
    }

    pub fn get<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        if let Some(value) = self.map.get(key) {
            Some(value)
        } else if let Some(parent) = self.parent {
            parent.get(key)
        } else {
            None
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.map.insert(key, value)
    }

    pub fn derive(&self, parent: &'a ChainMap<'a, K, V>) -> Self {
        Self {
            map: Default::default(),
            parent: Some(parent),
        }
    }
}
