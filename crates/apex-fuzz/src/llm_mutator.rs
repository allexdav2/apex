//! LLM-guided mutator — uses cached LLM responses for structured input mutation.
//!
//! When byte-level mutations stall on structured inputs, the LLM can produce
//! semantically valid variants. Results are cached to amortize LLM latency.

use std::collections::{HashMap, VecDeque};

/// A bounded cache mapping input bytes to pre-computed LLM mutation variants.
pub struct MutationCache {
    cache: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    order: VecDeque<Vec<u8>>,
    capacity: usize,
}

impl MutationCache {
    pub fn new(capacity: usize) -> Self {
        MutationCache {
            cache: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    /// Insert a set of mutation variants for a given input.
    pub fn insert(&mut self, input: Vec<u8>, variants: Vec<Vec<u8>>) {
        if self.cache.len() >= self.capacity && !self.cache.contains_key(&input) {
            // Evict oldest (O(1) with VecDeque).
            if let Some(oldest) = self.order.pop_front() {
                self.cache.remove(&oldest);
            }
        }
        if !self.cache.contains_key(&input) {
            self.order.push_back(input.clone());
        }
        self.cache.insert(input, variants);
    }

    /// Retrieve cached variants for an input.
    pub fn get(&self, input: &[u8]) -> Option<&Vec<Vec<u8>>> {
        self.cache.get(input)
    }
}

/// Format a mutation prompt for the LLM, including the input to mutate.
pub fn format_mutation_prompt(input: &[u8], format_hint: &str) -> String {
    let input_repr = if input
        .iter()
        .all(|b| b.is_ascii_graphic() || b.is_ascii_whitespace())
    {
        String::from_utf8_lossy(input).to_string()
    } else {
        format!(
            "hex: {}",
            input
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };

    format!(
        "You are a fuzzer mutation engine for {format_hint} inputs.\n\
         Generate 5 semantically valid variants of this input that explore \
         different code paths. Each variant should be syntactically valid {format_hint}.\n\n\
         Input:\n```\n{input_repr}\n```\n\n\
         Respond with each variant in a separate code block."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_mutator_returns_precomputed() {
        let mut cache = MutationCache::new(10);
        cache.insert(b"hello".to_vec(), vec![b"world".to_vec(), b"hi".to_vec()]);
        let variants = cache.get(b"hello");
        assert_eq!(variants.unwrap().len(), 2);
    }

    #[test]
    fn cache_miss_returns_none() {
        let cache = MutationCache::new(10);
        assert!(cache.get(b"unknown").is_none());
    }

    #[test]
    fn cache_evicts_oldest_at_capacity() {
        let mut cache = MutationCache::new(2);
        cache.insert(b"a".to_vec(), vec![b"a1".to_vec()]);
        cache.insert(b"b".to_vec(), vec![b"b1".to_vec()]);
        cache.insert(b"c".to_vec(), vec![b"c1".to_vec()]);
        // "a" should have been evicted.
        assert!(cache.get(b"a").is_none());
        assert!(cache.get(b"c").is_some());
    }

    #[test]
    fn format_mutation_prompt_includes_input() {
        let prompt = format_mutation_prompt(b"SELECT * FROM users", "sql");
        assert!(prompt.contains("SELECT"));
        assert!(prompt.contains("sql"));
    }

    #[test]
    fn format_mutation_prompt_binary_input() {
        let prompt = format_mutation_prompt(&[0xFF, 0x00, 0xAB], "binary");
        assert!(prompt.contains("hex"));
    }
}
