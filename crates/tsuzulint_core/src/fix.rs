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

    pub fn topological_sort<'a>(&self, rules: &[&'a str]) -> Result<Vec<&'a str>, String> {
        let mut in_degree: HashMap<&str, usize> = rules.iter().map(|r| (*r, 0)).collect();
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();

        for rule in rules {
            if let Some(deps) = self.dependencies.get(*rule) {
                for dep in deps {
                    if rules.contains(&dep.as_str()) {
                        graph.entry(dep.as_str()).or_default().push(*rule);
                        if let Some(degree) = in_degree.get_mut(*rule) {
                            *degree += 1;
                        } else {
                            return Err(format!("Rule '{}' not found in dependency graph", rule));
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
                    if let Some(degree) = in_degree.get_mut(next_rule) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(next_rule);
                        }
                    } else {
                        return Err(format!(
                            "Dependent rule '{}' not found in dependency graph",
                            next_rule
                        ));
                    }
                }
            }
        }
        Ok(result)
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
        let sorted = graph.topological_sort(&rules).unwrap();

        let md007_idx = sorted.iter().position(|&r| r == "MD007").unwrap();
        let md010_idx = sorted.iter().position(|&r| r == "MD010").unwrap();
        let md064_idx = sorted.iter().position(|&r| r == "MD064").unwrap();

        assert!(md007_idx < md010_idx);
        assert!(md010_idx < md064_idx);
    }

    #[test]
    fn test_dependency_graph_default() {
        let graph1 = DependencyGraph::new();
        let graph2 = DependencyGraph::default();
        assert_eq!(graph1.dependencies.len(), graph2.dependencies.len());
    }

    #[test]
    fn test_fix_coordinator_default() {
        let coord1 = FixCoordinator::new();
        let coord2 = FixCoordinator::default();
        assert_eq!(coord1.max_iterations, coord2.max_iterations);
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
