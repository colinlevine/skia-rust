//! Safe Rust registry sorting & lookup logic for `SkFlattenable`.

/// A registry entry storing name bytes, an opaque C name pointer token, and an opaque factory token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenableEntry {
    pub name: Vec<u8>,
    pub name_token: usize,
    pub factory_token: usize,
}

/// Safe Rust flattenable registry mirroring `SkFlattenable`'s global static table.
#[derive(Debug, Default)]
pub struct FlattenableRegistry {
    entries: Vec<FlattenableEntry>,
    sorted: bool,
}

impl FlattenableRegistry {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            sorted: false,
        }
    }

    pub fn register(&mut self, name: &[u8], name_token: usize, factory_token: usize) {
        self.entries.push(FlattenableEntry {
            name: name.to_vec(),
            name_token,
            factory_token,
        });
        self.sorted = false;
    }

    /// Sorts entries lexicographically by name bytes (matching `strcmp` ordering).
    /// Note: C `strcmp` compares as `unsigned char`, which matches Rust `&[u8]` comparison.
    pub fn finalize(&mut self) {
        self.entries.sort_by(|a, b| a.name.cmp(&b.name));
        self.sorted = true;
    }

    /// Binary-searches for `name`, returning the first matching entry's `factory_token`
    /// (matching `std::equal_range` returning `pair.first->fFactory`).
    pub fn name_to_factory(&self, name: &[u8]) -> Option<usize> {
        debug_assert!(self.sorted);
        let idx = self.entries.partition_point(|e| e.name.as_slice() < name);
        if idx < self.entries.len() && self.entries[idx].name.as_slice() == name {
            Some(self.entries[idx].factory_token)
        } else {
            None
        }
    }

    /// Reverse-scans entries for `factory_token`, returning its original `name_token`
    /// (matching `SkFlattenable::FactoryToName`).
    pub fn factory_to_name(&self, factory_token: usize) -> Option<usize> {
        for entry in self.entries.iter().rev() {
            if entry.factory_token == factory_token {
                return Some(entry.name_token);
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_register_finalize_lookup() {
        let mut reg = FlattenableRegistry::new();
        reg.register(b"SkShaderB", 0x1002, 0x2002);
        reg.register(b"SkShaderA", 0x1001, 0x2001);
        reg.register(b"SkShaderC", 0x1003, 0x2003);
        reg.register(b"SkShaderA_Alias", 0x1004, 0x2001);
        reg.finalize();

        assert_eq!(reg.name_to_factory(b"SkShaderA"), Some(0x2001));
        assert_eq!(reg.name_to_factory(b"SkShaderB"), Some(0x2002));
        assert_eq!(reg.name_to_factory(b"SkShaderC"), Some(0x2003));
        assert_eq!(reg.name_to_factory(b"Missing"), None);

        // Reverse scan returns the last sorted entry with factory 0x2001 ("SkShaderA_Alias").
        assert_eq!(reg.factory_to_name(0x2001), Some(0x1004));
        assert_eq!(reg.factory_to_name(0x2002), Some(0x1002));
        assert_eq!(reg.factory_to_name(0x9999), None);
    }
}
