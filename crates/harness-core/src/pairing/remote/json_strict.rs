//! JSON member names are compared after unescaping, at every nesting level.
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::collections::HashSet;
use std::fmt;

struct Checked;

impl<'de> Deserialize<'de> for Checked {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(CheckVisitor)
    }
}

struct CheckVisitor;

impl<'de> Visitor<'de> for CheckVisitor {
    type Value = Checked;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("JSON without duplicate object members")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Checked, M::Error> {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate JSON member"));
            }
            map.next_value::<Checked>()?;
        }
        Ok(Checked)
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Checked, S::Error> {
        while seq.next_element::<Checked>()?.is_some() {}
        Ok(Checked)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_unit<E: de::Error>(self) -> Result<Checked, E> {
        Ok(Checked)
    }
}

pub(super) fn parse(bytes: &[u8], code: &str) -> Result<Value, String> {
    serde_json::from_slice::<Checked>(bytes).map_err(|_| code.to_string())?;
    serde_json::from_slice(bytes).map_err(|_| code.to_string())
}
