use datafrog::{Iteration, Relation};
use wasm_bindgen::prelude::*;
use web_sys::console;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    tracing_wasm::set_as_global_default();
}

#[wasm_bindgen]
pub struct DatafrogEngine {
    results: Vec<String>,
}

#[wasm_bindgen]
impl DatafrogEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console::log_1(&"Datafrog WASM engine initialized".into());
        DatafrogEngine {
            results: Vec::new(),
        }
    }

    /// Run a simple transitive closure example
    /// Given edges (A, B), compute all reachable pairs
    #[wasm_bindgen]
    pub fn run_transitive_closure(&mut self) -> String {
        console::log_1(&"Running transitive closure with Datafrog...".into());

        self.results.clear();

        // Create initial edges: A->B, B->C, C->D
        let edges = vec![
            (0u32, 1u32), // A -> B
            (1u32, 2u32), // B -> C
            (2u32, 3u32), // C -> D
            (0u32, 2u32), // A -> C (direct)
        ];

        let mut iteration = Iteration::new();

        // Create relations
        let edges_relation = iteration.variable::<(u32, u32)>("edges");
        let reachable_relation = iteration.variable::<(u32, u32)>("reachable");

        // Seed with initial edges
        edges_relation.insert(Relation::from_vec(edges.clone()));

        // Fixed-point iteration
        while iteration.changed() {
            // reachable(X, Y) :- edges(X, Y)
            reachable_relation.from_map(&edges_relation, |&(x, y)| (x, y));

            // reachable(X, Z) :- reachable(X, Y), edges(Y, Z)
            reachable_relation
                .from_join(&reachable_relation, &edges_relation, |&x, &_y, &z| (x, z));
        }

        // Collect results
        let reachable = reachable_relation.complete();

        for &(from, to) in reachable.iter() {
            let result = format!("{} can reach {}", from, to);
            console::log_1(&result.clone().into());
            self.results.push(result);
        }

        console::log_1(&format!("Found {} reachable pairs", self.results.len()).into());

        self.results.join("\n")
    }

    /// Run a friend-of-friend query (social graph example)
    #[wasm_bindgen]
    pub fn run_friend_of_friend(&mut self) -> String {
        console::log_1(&"Running friend-of-friend query...".into());

        self.results.clear();

        // Friend relationships: (user_id, friend_id)
        let friends = vec![
            (1u32, 2u32), // Alice -> Bob
            (2u32, 3u32), // Bob -> Carol
            (3u32, 4u32), // Carol -> Dave
            (1u32, 5u32), // Alice -> Eve
            (5u32, 6u32), // Eve -> Frank
        ];

        let mut iteration = Iteration::new();

        let friend_relation = iteration.variable::<(u32, u32)>("friend");
        let fof_relation = iteration.variable::<(u32, u32)>("friend_of_friend");

        friend_relation.insert(Relation::from_vec(friends));

        while iteration.changed() {
            // friend_of_friend(A, C) :- friend(A, B), friend(B, C), A != C
            fof_relation.from_join(&friend_relation, &friend_relation, |&a, &_b, &c| {
                if a != c {
                    (a, c)
                } else {
                    (0, 0)
                }
            });
        }

        let fof = fof_relation.complete();

        for &(user, fof_user) in fof.iter() {
            let result = format!("User {} has friend-of-friend {}", user, fof_user);
            console::log_1(&result.clone().into());
            self.results.push(result);
        }

        console::log_1(
            &format!(
                "Found {} friend-of-friend relationships",
                self.results.len()
            )
            .into(),
        );

        self.results.join("\n")
    }

    /// Get the results from the last query
    #[wasm_bindgen]
    pub fn get_results(&self) -> String {
        self.results.join("\n")
    }

    /// Clear all results
    #[wasm_bindgen]
    pub fn clear(&mut self) {
        self.results.clear();
        console::log_1(&"Results cleared".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_datafrog_basic() {
        let mut engine = DatafrogEngine::new();
        let results = engine.run_transitive_closure();
        assert!(!results.is_empty());
    }
}
