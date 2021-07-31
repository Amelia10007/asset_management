use std::iter::FromIterator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpQuery<K, V> {
    queries: Vec<(K, V)>,
}

impl<K, V> HttpQuery<K, V> {
    pub fn empty() -> Self {
        std::iter::empty().collect()
    }

    pub fn get(&self, key: &K) -> Option<&V>
    where
        K: PartialEq,
    {
        self.queries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn build_query(&self) -> String
    where
        K: ToString,
        V: ToString,
    {
        let mut iter = self
            .queries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .map(|(ks, vs)| format!("{}={}", ks, vs));

        let mut s = iter.next().unwrap_or(String::new());

        for q in iter {
            s.push('&');
            s.extend(q.chars());
        }

        s
    }

    pub fn as_slice(&self) -> &[(K, V)] {
        self.queries.as_slice()
    }
}

impl<'a> HttpQuery<&'a str, &'a str> {
    pub fn parse(query_str: &'a str) -> HttpQuery<&'a str, &'a str> {
        query_str
            .split('&')
            .filter_map(|split| {
                let mut iter = split.split('=');
                match (iter.next(), iter.next()) {
                    (Some(key), Some(value)) => Some((key, value)),
                    _ => None,
                }
            })
            .collect()
    }
}

impl<K, V> FromIterator<(K, V)> for HttpQuery<K, V> {
    fn from_iter<A>(iter: A) -> Self
    where
        A: IntoIterator<Item = (K, V)>,
    {
        Self {
            queries: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_query_string_empty() {
        let q = HttpQuery::<String, String>::from_iter(vec![]);
        let query = q.build_query();

        assert_eq!("", &query);
    }

    #[test]
    fn test_parse() {
        let q = HttpQuery::parse("key=1&answer=42");

        assert_eq!(Some(&"1"), q.get(&"key"));
        assert_eq!(Some(&"42"), q.get(&"answer"));
    }

    #[test]
    fn test_to_query_string_single() {
        let q = HttpQuery::from_iter(vec![("key", "value")]);
        let query = q.build_query();

        assert_eq!("key=value", &query);
    }

    #[test]
    fn test_to_query_string_double() {
        let q = HttpQuery::from_iter(vec![("key", 1), ("answer", 42)]);
        let query = q.build_query();

        assert_eq!("key=1&answer=42", &query);
    }
}
