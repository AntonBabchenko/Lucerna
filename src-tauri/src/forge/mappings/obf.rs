//! `ObfIndex` — two-way lookup keyed by:
//!   - class:  obf-FQN → mapped-FQN
//!   - field:  (obf-FQN, obf-name) → mapped-name
//!   - method: (obf-FQN, obf-name, descriptor) → mapped-name
//!
//! "FQN" uses `/` as separator (JVM internal class names).

use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct ObfIndex {
    classes: HashMap<String, String>,
    fields: HashMap<(String, String), String>,
    methods: HashMap<(String, String, String), String>,
}

impl ObfIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put_class(&mut self, obf: &str, mapped: &str) {
        self.classes.insert(obf.into(), mapped.into());
    }
    pub fn put_field(&mut self, obf_class: &str, obf_name: &str, mapped_name: &str) {
        self.fields
            .insert((obf_class.into(), obf_name.into()), mapped_name.into());
    }
    pub fn put_method(&mut self, obf_class: &str, obf_name: &str, desc: &str, mapped_name: &str) {
        self.methods.insert(
            (obf_class.into(), obf_name.into(), desc.into()),
            mapped_name.into(),
        );
    }

    pub fn lookup_class(&self, obf: &str) -> Option<String> {
        self.classes.get(obf).cloned()
    }
    pub fn lookup_field(&self, obf_class: &str, obf_name: &str) -> Option<String> {
        self.fields
            .get(&(obf_class.into(), obf_name.into()))
            .cloned()
    }
    pub fn lookup_method(&self, obf_class: &str, obf_name: &str, desc: &str) -> Option<String> {
        self.methods
            .get(&(obf_class.into(), obf_name.into(), desc.into()))
            .cloned()
    }

    pub fn class_count(&self) -> usize {
        self.classes.len()
    }
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }
    pub fn method_count(&self) -> usize {
        self.methods.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_index_lookups_return_none() {
        let idx = ObfIndex::new();
        assert!(idx.lookup_class("a").is_none());
        assert!(idx.lookup_field("a", "b").is_none());
        assert!(idx.lookup_method("a", "b", "()V").is_none());
    }

    #[test]
    fn round_trip_class_field_method() {
        let mut idx = ObfIndex::new();
        idx.put_class("a/b/C", "com/x/Class");
        idx.put_field("a/b/C", "f", "fieldName");
        idx.put_method("a/b/C", "m", "(I)V", "methodName");
        assert_eq!(idx.lookup_class("a/b/C").as_deref(), Some("com/x/Class"));
        assert_eq!(idx.lookup_field("a/b/C", "f").as_deref(), Some("fieldName"));
        assert_eq!(
            idx.lookup_method("a/b/C", "m", "(I)V").as_deref(),
            Some("methodName")
        );
    }

    #[test]
    fn method_lookup_is_descriptor_sensitive() {
        let mut idx = ObfIndex::new();
        idx.put_method("a/b/C", "m", "()V", "noArgs");
        idx.put_method("a/b/C", "m", "(I)V", "intArg");
        assert_eq!(
            idx.lookup_method("a/b/C", "m", "()V").as_deref(),
            Some("noArgs")
        );
        assert_eq!(
            idx.lookup_method("a/b/C", "m", "(I)V").as_deref(),
            Some("intArg")
        );
    }
}
