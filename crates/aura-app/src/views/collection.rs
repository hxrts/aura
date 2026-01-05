//! # Domain Collection
//!
//! A typesafe wrapper for domain state collections that enforces explicit ID-based access.
//!
//! This module provides [`DomainCollection`], a generic container that:
//! - Stores items by ID in a HashMap for O(1) lookup
//! - Exposes consistent query methods (`get`, `all`, `count`, etc.)
//! - Exposes consistent mutation methods (`apply`, `remove`, `update`, etc.)
//! - Prevents UI state (selection, filters) from leaking into domain types
//!
//! ## Example
//!
//! ```rust,ignore
//! use aura_app::views::DomainCollection;
//!
//! pub struct ContactsState {
//!     contacts: DomainCollection<AuthorityId, Contact>,
//! }
//!
//! // Access always requires explicit ID
//! let contact = state.contacts.get(&authority_id);
//! state.contacts.apply(authority_id, new_contact);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::Hash;

/// A typesafe domain collection that enforces explicit ID-based access.
///
/// This wrapper provides consistent query and mutation patterns across all
/// domain state types, eliminating boilerplate and preventing UI state
/// (selection, filters) from leaking into domain types.
///
/// # Type Parameters
///
/// - `Id`: The identifier type (must be `Eq + Hash + Clone`)
/// - `Item`: The item type stored in the collection
///
/// # Design Principles
///
/// 1. **No selection state**: Selection is a UI concern, not stored here
/// 2. **Explicit context**: All access requires an ID parameter
/// 3. **Consistent API**: Same methods across all domain state types
/// 4. **Computed properties**: Counts are derived, never stored separately
#[derive(Debug, Clone)]
pub struct DomainCollection<Id, Item>
where
    Id: Eq + Hash + Clone,
{
    items: HashMap<Id, Item>,
}

impl<Id, Item> Default for DomainCollection<Id, Item>
where
    Id: Eq + Hash + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Id, Item> DomainCollection<Id, Item>
where
    Id: Eq + Hash + Clone,
{
    /// Create an empty collection.
    pub fn new() -> Self {
        Self {
            items: HashMap::new(),
        }
    }

    /// Create a collection with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: HashMap::with_capacity(capacity),
        }
    }

    /// Create a collection from an iterator of (id, item) pairs.
    pub fn from_pairs(iter: impl IntoIterator<Item = (Id, Item)>) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }

    // ─── Queries ─────────────────────────────────────────────

    /// Get an item by ID.
    pub fn get(&self, id: &Id) -> Option<&Item> {
        self.items.get(id)
    }

    /// Get a mutable reference to an item by ID.
    pub fn get_mut(&mut self, id: &Id) -> Option<&mut Item> {
        self.items.get_mut(id)
    }

    /// Check if an item exists.
    pub fn contains(&self, id: &Id) -> bool {
        self.items.contains_key(id)
    }

    /// Get all items as an iterator.
    pub fn all(&self) -> impl Iterator<Item = &Item> {
        self.items.values()
    }

    /// Get all (id, item) pairs as an iterator.
    pub fn iter(&self) -> impl Iterator<Item = (&Id, &Item)> {
        self.items.iter()
    }

    /// Get all IDs as an iterator.
    pub fn ids(&self) -> impl Iterator<Item = &Id> {
        self.items.keys()
    }

    /// Get the count of items (computed, not stored).
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Check if the collection is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    // ─── Mutations ───────────────────────────────────────────

    /// Insert or replace an item. Returns the previous item if it existed.
    pub fn apply(&mut self, id: Id, item: Item) -> Option<Item> {
        self.items.insert(id, item)
    }

    /// Remove an item, returning it if it existed.
    pub fn remove(&mut self, id: &Id) -> Option<Item> {
        self.items.remove(id)
    }

    /// Update an item in place. Returns `true` if the item existed and was updated.
    pub fn update(&mut self, id: &Id, f: impl FnOnce(&mut Item)) -> bool {
        if let Some(item) = self.items.get_mut(id) {
            f(item);
            true
        } else {
            false
        }
    }

    /// Update an item or return an error if not found.
    ///
    /// This is useful when you want to surface "not found" errors to the caller
    /// instead of silently doing nothing.
    pub fn try_update<E>(
        &mut self,
        id: &Id,
        f: impl FnOnce(&mut Item),
        not_found: impl FnOnce() -> E,
    ) -> Result<(), E> {
        if self.update(id, f) {
            Ok(())
        } else {
            Err(not_found())
        }
    }

    /// Get or insert a default item, then return a mutable reference.
    pub fn get_or_insert_with(&mut self, id: Id, default: impl FnOnce() -> Item) -> &mut Item {
        self.items.entry(id).or_insert_with(default)
    }

    /// Clear all items from the collection.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Retain only items matching a predicate.
    pub fn retain(&mut self, f: impl FnMut(&Id, &mut Item) -> bool) {
        self.items.retain(f);
    }

    /// Extend the collection with items from an iterator.
    pub fn extend(&mut self, iter: impl IntoIterator<Item = (Id, Item)>) {
        self.items.extend(iter);
    }
}

// ─── Serde Support ───────────────────────────────────────────

impl<Id, Item> Serialize for DomainCollection<Id, Item>
where
    Id: Eq + Hash + Clone + Serialize,
    Item: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.items.serialize(serializer)
    }
}

impl<'de, Id, Item> Deserialize<'de> for DomainCollection<Id, Item>
where
    Id: Eq + Hash + Clone + Deserialize<'de>,
    Item: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let items = HashMap::deserialize(deserializer)?;
        Ok(Self { items })
    }
}

// ─── Conversion Traits ───────────────────────────────────────

impl<Id, Item> FromIterator<(Id, Item)> for DomainCollection<Id, Item>
where
    Id: Eq + Hash + Clone,
{
    fn from_iter<T: IntoIterator<Item = (Id, Item)>>(iter: T) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }
}

impl<Id, Item> IntoIterator for DomainCollection<Id, Item>
where
    Id: Eq + Hash + Clone,
{
    type Item = (Id, Item);
    type IntoIter = std::collections::hash_map::IntoIter<Id, Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, Id, Item> IntoIterator for &'a DomainCollection<Id, Item>
where
    Id: Eq + Hash + Clone,
{
    type Item = (&'a Id, &'a Item);
    type IntoIter = std::collections::hash_map::Iter<'a, Id, Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_and_default() {
        let coll1: DomainCollection<String, i32> = DomainCollection::new();
        let coll2: DomainCollection<String, i32> = DomainCollection::default();

        assert!(coll1.is_empty());
        assert!(coll2.is_empty());
        assert_eq!(coll1.count(), 0);
    }

    #[test]
    fn test_apply_and_get() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();

        // Insert new item
        let prev = coll.apply("a".to_string(), 1);
        assert!(prev.is_none());
        assert_eq!(coll.get(&"a".to_string()), Some(&1));

        // Replace existing item
        let prev = coll.apply("a".to_string(), 10);
        assert_eq!(prev, Some(1));
        assert_eq!(coll.get(&"a".to_string()), Some(&10));
    }

    #[test]
    fn test_remove() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();
        coll.apply("a".to_string(), 1);
        coll.apply("b".to_string(), 2);

        let removed = coll.remove(&"a".to_string());
        assert_eq!(removed, Some(1));
        assert_eq!(coll.count(), 1);
        assert!(!coll.contains(&"a".to_string()));

        let removed = coll.remove(&"missing".to_string());
        assert!(removed.is_none());
    }

    #[test]
    fn test_update() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();
        coll.apply("a".to_string(), 1);

        // Update existing
        let updated = coll.update(&"a".to_string(), |v| *v = 10);
        assert!(updated);
        assert_eq!(coll.get(&"a".to_string()), Some(&10));

        // Update non-existing
        let updated = coll.update(&"missing".to_string(), |v| *v = 10);
        assert!(!updated);
    }

    #[test]
    fn test_try_update() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();
        coll.apply("a".to_string(), 1);

        // Success case
        let result: Result<(), &str> =
            coll.try_update(&"a".to_string(), |v| *v = 10, || "not found");
        assert!(result.is_ok());
        assert_eq!(coll.get(&"a".to_string()), Some(&10));

        // Error case
        let result: Result<(), &str> =
            coll.try_update(&"missing".to_string(), |v| *v = 10, || "not found");
        assert_eq!(result, Err("not found"));
    }

    #[test]
    fn test_get_or_insert_with() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();

        // Insert new
        let val = coll.get_or_insert_with("a".to_string(), || 42);
        assert_eq!(*val, 42);

        // Get existing (doesn't call closure)
        let val = coll.get_or_insert_with("a".to_string(), || panic!("should not be called"));
        assert_eq!(*val, 42);
    }

    #[test]
    fn test_all_and_iter() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();
        coll.apply("a".to_string(), 1);
        coll.apply("b".to_string(), 2);
        coll.apply("c".to_string(), 3);

        let mut values: Vec<_> = coll.all().copied().collect();
        values.sort();
        assert_eq!(values, vec![1, 2, 3]);

        let mut ids: Vec<_> = coll.ids().cloned().collect();
        ids.sort();
        assert_eq!(ids, vec!["a", "b", "c"]);

        assert_eq!(coll.count(), 3);
    }

    #[test]
    fn test_retain() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();
        coll.apply("a".to_string(), 1);
        coll.apply("b".to_string(), 2);
        coll.apply("c".to_string(), 3);

        coll.retain(|_, v| *v > 1);
        assert_eq!(coll.count(), 2);
        assert!(!coll.contains(&"a".to_string()));
        assert!(coll.contains(&"b".to_string()));
        assert!(coll.contains(&"c".to_string()));
    }

    #[test]
    fn test_from_pairs_and_from_iter() {
        let pairs = vec![("a".to_string(), 1), ("b".to_string(), 2)];

        let coll1 = DomainCollection::from_pairs(pairs.clone());
        assert_eq!(coll1.count(), 2);

        let coll2: DomainCollection<String, i32> = pairs.into_iter().collect();
        assert_eq!(coll2.count(), 2);
    }

    #[test]
    fn test_into_iter() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();
        coll.apply("a".to_string(), 1);
        coll.apply("b".to_string(), 2);

        let mut pairs: Vec<_> = coll.into_iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(pairs, vec![("a".to_string(), 1), ("b".to_string(), 2)]);
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();
        coll.apply("a".to_string(), 1);
        coll.apply("b".to_string(), 2);

        let json = serde_json::to_string(&coll).unwrap();
        let restored: DomainCollection<String, i32> = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.count(), 2);
        assert_eq!(restored.get(&"a".to_string()), Some(&1));
        assert_eq!(restored.get(&"b".to_string()), Some(&2));
    }

    #[test]
    fn test_clear() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();
        coll.apply("a".to_string(), 1);
        coll.apply("b".to_string(), 2);

        coll.clear();
        assert!(coll.is_empty());
        assert_eq!(coll.count(), 0);
    }

    #[test]
    fn test_extend() {
        let mut coll: DomainCollection<String, i32> = DomainCollection::new();
        coll.apply("a".to_string(), 1);

        coll.extend(vec![("b".to_string(), 2), ("c".to_string(), 3)]);

        assert_eq!(coll.count(), 3);
        assert_eq!(coll.get(&"b".to_string()), Some(&2));
        assert_eq!(coll.get(&"c".to_string()), Some(&3));
    }
}
