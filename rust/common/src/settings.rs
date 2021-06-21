use apply::Apply;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Error, Read};
use std::path::Path;

#[derive(Debug)]
pub struct Settings {
    map: HashMap<String, String>,
}

impl Settings {
    pub fn new() -> Settings {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn parse<S: AsRef<str>>(text: S) -> Settings {
        text.as_ref()
            .lines()
            .filter_map(|line| {
                // Ignore leading/trailing whitespace
                let line = line.trim();

                // Filter comment line
                if line.starts_with('#') {
                    None
                } else {
                    // Valid format: key=value
                    let mut iter = line.split('=');
                    if let (Some(key), Some(value)) = (iter.next(), iter.next()) {
                        Some((key.to_string(), value.to_string()))
                    } else {
                        None
                    }
                }
            })
            .collect::<HashMap<_, _>>()
            .apply(|map| Settings { map })
    }

    pub fn read_from<P: AsRef<Path>>(path: P) -> Result<Settings, Error> {
        File::open(path)?
            .apply(|mut f| {
                let mut s = String::new();
                f.read_to_string(&mut s).map(|_| s)
            })?
            .apply(Self::parse)
            .apply(Ok)
    }

    pub fn get<S: AsRef<str>>(&self, key: S) -> Option<&str> {
        self.map.get(key.as_ref()).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        let s = r"
    #comment=ignored
key=value
    answer=42 
    unspecifiedkey=
    =invaluevalue

    ignoredline";

        let settings = Settings::parse(s);

        assert_eq!(Some("value"), settings.get("key"));
        assert_eq!(Some("42"), settings.get("answer"));
        assert_eq!(Some(""), settings.get("unspecifiedkey"));
        assert_eq!(None, settings.get("comment"));
        assert_eq!(None, settings.get("invalidkey"));
    }
}
