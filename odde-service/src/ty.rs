use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct User {
    pub name: String,
    pub keys: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]

pub struct Config {
    #[serde(deserialize_with = "deser_config_users")]
    pub users: Vec<User>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UTaskRequest {
    pub key: String
}

fn deser_config_users<'de, D>(deserializer: D) -> Result<Vec<User>, D::Error>
where
    D: Deserializer<'de>,
{
    let m = HashMap::<String, Vec<String>>::deserialize(deserializer)?;
    Ok(m.into_iter()
        .map(|(name, keys)| User { name, keys })
        .collect())
}
