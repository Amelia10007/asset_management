use std::collections::{HashMap, HashSet};
use std::hash::Hash;

pub struct ExchangeGraph<T> {
    rates: HashMap<(T, T), f64>,
    direct_relations: HashMap<T, Vec<T>>,
}

impl<T> ExchangeGraph<T> {
    pub fn from_rates(rates: impl IntoIterator<Item = (T, T, f64)>) -> Self
    where
        T: Copy + Eq + Hash,
    {
        let mut rate_map = HashMap::new();
        let mut direct_relations = HashMap::new();

        for (base, target, rate) in rates.into_iter() {
            // Register relationships bi-directionally
            rate_map.insert((base, target), rate);
            rate_map.insert((target, base), 1.0 / rate);

            direct_relations
                .entry(base)
                .and_modify(|v: &mut Vec<_>| v.push(target))
                .or_insert(vec![target]);
            direct_relations
                .entry(target)
                .and_modify(|v| v.push(base))
                .or_insert(vec![base]);
        }

        Self {
            rates: rate_map,
            direct_relations,
        }
    }

    pub fn rate_between(&mut self, base: T, target: T) -> Option<f64>
    where
        T: Copy + Eq + Hash,
    {
        self.rate_between_inner(base, target, HashSet::new())
    }

    fn rate_between_inner(&mut self, base: T, target: T, appeared_ids: HashSet<T>) -> Option<f64>
    where
        T: Copy + Eq + Hash,
    {
        if let Some(rate) = self.rate_inner(base, target) {
            return Some(rate);
        }

        for intermediate in self
            .direct_relations
            .get(&base)
            .iter()
            .flat_map(|v| v.into_iter())
            .filter(|intermediate| !appeared_ids.contains(intermediate))
            .copied()
            .collect::<Vec<_>>()
            .into_iter()
        {
            if let Some(&rate1) = self.rates.get(&(base, intermediate)) {
                let mut appeared_ids = appeared_ids.clone();
                appeared_ids.insert(intermediate);
                if let Some(rate2) = self.rate_between_inner(intermediate, target, appeared_ids) {
                    //
                    let rate = rate1 * rate2;
                    // Register search result for faster re-search
                    self.rates.insert((base, target), rate);
                    self.direct_relations
                        .entry(base)
                        .and_modify(|v| v.push(target))
                        .or_insert(vec![target]);
                    //
                    return Some(rate);
                }
            }
        }

        None
    }

    fn rate_inner(&self, base: T, target: T) -> Option<f64>
    where
        T: Copy + Eq + Hash,
    {
        if base == target {
            Some(1.0)
        } else if let Some(&rate) = self.rates.get(&(base, target)) {
            Some(rate)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExchangeGraph;

    #[test]
    fn test_rate_between_neighbor() {
        let rates = vec![("a", "b", 10.0)];

        let mut graph = ExchangeGraph::from_rates(rates);

        let forward_rate = graph.rate_between("a", "b");
        let backward_rate = graph.rate_between("b", "a");

        assert_eq!(Some(10.0), forward_rate);
        assert_eq!(Some(0.1), backward_rate);
    }

    #[test]
    fn test_rate_between_neighbors() {
        let rates = vec![("a", "b", 10.0), ("a", "c", 2.0)];

        let mut graph = ExchangeGraph::from_rates(rates);

        let forward_rate = graph.rate_between("a", "b");
        let backward_rate = graph.rate_between("b", "a");
        assert_eq!(Some(10.0), forward_rate);
        assert_eq!(Some(0.1), backward_rate);

        let forward_rate = graph.rate_between("a", "c");
        let backward_rate = graph.rate_between("c", "a");
        assert_eq!(Some(2.0), forward_rate);
        assert_eq!(Some(0.5), backward_rate);
    }

    #[test]
    fn test_rate_between_equivalent() {
        let rates = vec![("a", "b", 10.0)];

        let mut graph = ExchangeGraph::from_rates(rates);

        assert_eq!(Some(1.0), graph.rate_between("a", "a"));
    }

    #[test]
    fn test_rate_between_empty() {
        let mut graph = ExchangeGraph::from_rates(std::iter::empty());

        assert_eq!(None, graph.rate_between("a", "b"));
    }

    #[test]
    fn test_rate_between_bridge3() {
        let rates = vec![("a", "b", 10.0), ("b", "c", 2.0)]; // 1a=10b, 1b=2c

        let mut graph = ExchangeGraph::from_rates(rates);
        let forward_rate = graph.rate_between("a", "c");
        let backward_rate = graph.rate_between("c", "a");

        assert_eq!(Some(20.0), forward_rate);
        assert_eq!(Some(0.05), backward_rate);
    }

    #[test]
    fn test_rate_between_bridge3_with_inverse() {
        let rates = vec![("a", "b", 10.0), ("c", "b", 0.5)]; // 1a=10b, 1b=2c

        let mut graph = ExchangeGraph::from_rates(rates);
        let forward_rate = graph.rate_between("a", "c");
        let backward_rate = graph.rate_between("c", "a");

        assert_eq!(Some(20.0), forward_rate);
        assert_eq!(Some(0.05), backward_rate);
    }

    #[test]
    fn test_rate_between_bridge3_start_from_center() {
        let rates = vec![("b", "a", 0.1), ("b", "c", 2.0)]; // 1a=10b, 1b=2c

        let mut graph = ExchangeGraph::from_rates(rates);

        let forward_rate = graph.rate_between("a", "c");
        let backward_rate = graph.rate_between("c", "a");

        assert_eq!(Some(20.0), forward_rate);
        assert_eq!(Some(0.05), backward_rate);
    }

    #[test]
    fn test_rate_between_bridge4() {
        let rates = vec![("a", "b", 10.0), ("b", "c", 2.0), ("c", "d", 4.0)];

        let mut graph = ExchangeGraph::from_rates(rates);
        let forward_rate = graph.rate_between("a", "d");
        let backward_rate = graph.rate_between("d", "a");

        assert_eq!(Some(80.0), forward_rate);
        assert_eq!(Some(0.0125), backward_rate);
    }

    #[test]
    fn test_rate_between_isolated_clusters() {
        let rates = vec![("a", "b", 10.0), ("b", "c", 2.0), ("foo", "bar", 4.0)];

        let mut graph = ExchangeGraph::from_rates(rates);

        let rate = graph.rate_between("a", "foo");

        assert_eq!(None, rate);
    }
}
