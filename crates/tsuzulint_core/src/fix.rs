use blake3::Hash;
use std::collections::HashMap;

pub struct DependencyGraph {
    dependencies: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        let mut deps = HashMap::new();
        deps.insert("MD064".into(), vec!["MD010".into()]);
        deps.insert("MD010".into(), vec!["MD007".into()]);
        Self { dependencies: deps }
    }

    pub fn topological_sort<'a>(&self, rules: &[&'a str]) -> Vec<&'a str> {
        let mut in_degree: HashMap<&str, usize> = rules.iter().map(|r| (*r, 0)).collect();
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();

        for rule in rules {
            if let Some(deps) = self.dependencies.get(*rule) {
                for dep in deps {
                    if rules.contains(&dep.as_str()) {
                        graph.entry(dep.as_str()).or_default().push(*rule);
                        if let Some(count) = in_degree.get_mut(*rule) {
                            *count += 1;
                        }
                    }
                }
            }
        }

        let mut queue: Vec<&str> = in_degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(r, _)| *r)
            .collect();

        let mut result = Vec::new();
        while let Some(rule) = queue.pop() {
            result.push(rule);
            if let Some(next) = graph.get(rule) {
                for &next_rule in next {
                    if let Some(count) = in_degree.get_mut(next_rule) {
                        *count -= 1;
                        if *count == 0 {
                            queue.push(next_rule);
                        }
                    }
                }
            }
        }
        result
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum FixResult {
    Converged { iterations: usize },
    MaxIterationsReached { remaining: usize },
    CycleDetected { cycle_length: usize },
}

pub struct FixCoordinator {
    #[allow(dead_code)]
    graph: DependencyGraph,
    max_iterations: usize,
}

impl FixCoordinator {
    pub fn new() -> Self {
        Self {
            graph: DependencyGraph::new(),
            max_iterations: 3,
        }
    }

    pub fn apply_fixes_iterative<F>(&self, content: &mut String, mut apply_fix: F) -> FixResult
    where
        F: FnMut(&str) -> Option<String>,
    {
        let mut history: Vec<Hash> = vec![self.hash_content(content)];
        let mut iterations = 0;

        while iterations < self.max_iterations {
            iterations += 1;

            if let Some(fixed) = apply_fix(content) {
                *content = fixed;
                let current_hash = self.hash_content(content);

                if let Some(prev_idx) = history.iter().position(|h| *h == current_hash) {
                    return FixResult::CycleDetected {
                        cycle_length: history.len() - prev_idx,
                    };
                }
                history.push(current_hash);
            } else {
                return FixResult::Converged { iterations };
            }
        }

        FixResult::MaxIterationsReached { remaining: 0 }
    }

    fn hash_content(&self, content: &str) -> Hash {
        blake3::hash(content.as_bytes())
    }
}

impl Default for FixCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topological_sort() {
        let graph = DependencyGraph::new();
        let rules = vec!["MD064", "MD010", "MD007"];
        let sorted = graph.topological_sort(&rules);

        let md007_idx = sorted.iter().position(|&r| r == "MD007").unwrap();
        let md010_idx = sorted.iter().position(|&r| r == "MD010").unwrap();
        let md064_idx = sorted.iter().position(|&r| r == "MD064").unwrap();

        assert!(md007_idx < md010_idx);
        assert!(md010_idx < md064_idx);
    }

    #[test]
    fn test_topological_sort_missing_in_degree() {
        // Test what happens if the graph logic goes wrong and a rule is missing
        // from in_degree map. Since topological_sort constructs the map using `rules`,
        // a standard input won't trigger the missing map entry.
        // But we can artificially simulate a situation where it's safe by verifying it
        // doesn't panic.
        let mut graph = DependencyGraph::new();
        // Add a dependency to a completely unknown rule
        graph
            .dependencies
            .insert("UnknownA".into(), vec!["UnknownB".into()]);

        // Include "UnknownA", but not "UnknownB"
        let rules = vec!["UnknownA"];
        let sorted = graph.topological_sort(&rules);
        assert_eq!(sorted, vec!["UnknownA"]);

        // Include both
        let rules_both = vec!["UnknownA", "UnknownB"];
        let sorted_both = graph.topological_sort(&rules_both);

        let idx_b = sorted_both.iter().position(|&r| r == "UnknownB").unwrap();
        let idx_a = sorted_both.iter().position(|&r| r == "UnknownA").unwrap();
        assert!(idx_b < idx_a);
    }

    #[test]
    fn test_cycle_detection() {
        let coordinator = FixCoordinator::new();
        let mut content = "a".to_string();
        let mut call_count = 0;

        let result = coordinator.apply_fixes_iterative(&mut content, |_| {
            call_count += 1;
            if call_count % 2 == 1 {
                Some("b".to_string())
            } else {
                Some("a".to_string())
            }
        });

        assert!(matches!(result, FixResult::CycleDetected { .. }));
    }

    #[test]
    fn test_convergence() {
        let coordinator = FixCoordinator::new();
        let mut content = "a".to_string();
        let mut call_count = 0;

        let result = coordinator.apply_fixes_iterative(&mut content, |_| {
            call_count += 1;
            if call_count < 2 {
                Some("b".to_string())
            } else {
                None
            }
        });

        assert!(matches!(result, FixResult::Converged { .. }));
    }
}
