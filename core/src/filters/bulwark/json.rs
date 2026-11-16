//! A JSON value that keeps the order of an object's keys, so that Bulwark's metadata can be
//! read and written back byte for byte (`serde_json::Value` sorts keys).

use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

#[derive(Debug, Clone, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(serde_json::Number),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn parse(text: &str) -> Option<J> {
        serde_json::from_str(text).ok()
    }

    pub fn str(s: impl Into<String>) -> J {
        J::Str(s.into())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            J::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[J]> {
        match self {
            J::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_obj(&self) -> Option<&[(String, J)]> {
        match self {
            J::Obj(o) => Some(o),
            _ => None,
        }
    }

    /// The value of `key` when this is an object.
    pub fn get(&self, key: &str) -> Option<&J> {
        self.as_obj()?.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// JavaScript truthiness, for the few places Bulwark tests a value with `?` or `||`.
    pub fn truthy(&self) -> bool {
        match self {
            J::Null => false,
            J::Bool(b) => *b,
            J::Num(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
            J::Str(s) => !s.is_empty(),
            J::Arr(_) | J::Obj(_) => true,
        }
    }

    /// One-line JSON as `JSON.stringify` writes it.
    pub fn write(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Num(n) => out.push_str(&n.to_string()),
            J::Str(s) => out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())),
            J::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            J::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).unwrap_or_else(|_| "\"\"".into()));
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }
}

/// Sets `key` in an object's pairs: in place when there, else at the end.
pub fn set(pairs: &mut Vec<(String, J)>, key: &str, value: J) {
    match pairs.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => *v = value,
        None => pairs.push((key.to_string(), value)),
    }
}

pub fn remove(pairs: &mut Vec<(String, J)>, key: &str) {
    pairs.retain(|(k, _)| k != key);
}

impl<'de> Deserialize<'de> for J {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<J, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = J;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON value")
            }

            fn visit_unit<E>(self) -> Result<J, E> {
                Ok(J::Null)
            }

            fn visit_none<E>(self) -> Result<J, E> {
                Ok(J::Null)
            }

            fn visit_bool<E>(self, b: bool) -> Result<J, E> {
                Ok(J::Bool(b))
            }

            fn visit_i64<E>(self, n: i64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }

            fn visit_u64<E>(self, n: u64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }

            fn visit_f64<E>(self, n: f64) -> Result<J, E> {
                Ok(serde_json::Number::from_f64(n).map(J::Num).unwrap_or(J::Null))
            }

            fn visit_str<E>(self, s: &str) -> Result<J, E> {
                Ok(J::Str(s.to_string()))
            }

            fn visit_string<E>(self, s: String) -> Result<J, E> {
                Ok(J::Str(s))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<J, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(J::Arr(items))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<J, A::Error> {
                let mut pairs: Vec<(String, J)> = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, J>()? {
                    // `JSON.parse` keeps the first position and the last value of a repeated key.
                    set(&mut pairs, &k, v);
                }
                Ok(J::Obj(pairs))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_kept_and_written_as_javascript_does() {
        let text = r#"{"b":1,"a":[true,null,"x\"y\n/é"],"c":{"z":0,"y":-2}}"#;
        let j = J::parse(text).unwrap();
        assert_eq!(j.to_json(), r#"{"b":1,"a":[true,null,"x\"y\n/é"],"c":{"z":0,"y":-2}}"#);
        assert!(J::parse("{").is_none());
    }
}
