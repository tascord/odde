use {
    serde::{Deserialize, Deserializer, Serialize, Serializer},
    std::collections::HashMap,
};

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub keys: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub vm: VmConfig,
    pub git: GitConfig,
    #[serde(deserialize_with = "deser_config_users", serialize_with = "ser_config_users")]
    pub users: Vec<User>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GitConfig {
    pub key: String,
    pub urls: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VmConfig {
    pub memory: f32,
    pub storage: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ODDERequest {
    pub key: String,
}

fn deser_config_users<'de, D>(deserializer: D) -> Result<Vec<User>, D::Error>
where
    D: Deserializer<'de>,
{
    let m = HashMap::<String, Vec<String>>::deserialize(deserializer)?;
    Ok(m.into_iter().map(|(name, keys)| User { name, keys }).collect())
}

fn ser_config_users<S>(users: &[User], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let m: HashMap<String, Vec<String>> = users.iter().map(|user| (user.name.clone(), user.keys.clone())).collect();
    m.serialize(serializer)
}
