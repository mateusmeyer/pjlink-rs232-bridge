use std::{collections::HashMap, convert::TryInto, fmt, fs};
use serde::{Deserialize, Deserializer, de::{self, MapAccess, Visitor}};

pub type BridgeDefinitionCommandsMap = HashMap<[u8; 5], BridgeDefinitionCommand>;
pub type BridgeDefinitionCommandDefinitionsMap = HashMap<Vec<u8>, BridgeDefinitionCommandDefinition>;

#[derive(Deserialize)]
pub struct BridgeDefinition {
    pub general: BridgeDefinitionGeneral,
    pub connection: BridgeDefinitionConnection,
    pub resolution: Option<BridgeDefinitionResolution>,
    pub behavior: Option<BridgeDefinitionBehavior>,
    #[serde(deserialize_with = "deserialize_bridge_commands")]
    pub commands: BridgeDefinitionCommandsMap
}

#[derive(Deserialize)]
pub struct BridgeDefinitionGeneral {
    pub manufacturer_name: String,
    pub product_name: String,
    pub software_version: String,
    pub class_type: u8,
}

#[derive(Deserialize)]
pub struct BridgeDefinitionConnection {
    pub baud_rate: u32,
    pub data_bits: Option<u8>,
    pub parity: Option<char>,
    pub stop_bits: Option<u8>,
    pub hardware_flow_control: Option<bool>,
    pub software_flow_control: Option<bool>
}

#[derive(Deserialize)]
pub struct BridgeDefinitionResolution {
    pub current: Option<[u32; 2]>,
    pub recommended: Option<[u32; 2]>
}

#[derive(Deserialize)]
pub struct BridgeDefinitionBehavior {
    pub send_on_start: Option<Vec<u8>>,
    pub wait_for_response: Option<u32>
}

#[derive(Deserialize)]
#[derive(Debug)]
pub struct BridgeDefinitionCommand {
    #[serde(deserialize_with = "deserialize_bridge_command_definition")]
    pub inputs: BridgeDefinitionCommandDefinitionsMap,
    pub wait_for_response: Option<u32>
}

#[derive(Deserialize)]
#[derive(Debug)]
pub struct BridgeDefinitionCommandDefinition {
    pub send: Vec<u8>,
    pub send_times: Option<u32>,
    pub send_timeout: Option<u32>,
    pub wait_for_response: Option<u32>,
    pub outputs: Vec<BridgeDefinitionCommandDefinitionOutput>
}

#[derive(Deserialize, Debug)]
#[serde(tag = "response_type", content = "response_value", rename_all = "lowercase")]
pub enum BridgeDefinitionCommandDefinitionOutputResponse {
    Default(String),
    Value(String)
} 

#[derive(Deserialize, Debug)]
#[serde(tag = "on_received_type", content = "on_received", rename_all = "snake_case")]
pub enum BridgeDefinitionCommandDefinitionOutputProjectorResponse {
    Value(Vec<u8>),
    RuleMap(
        BridgeDefinitionCommandDefinitionOutputProjectorResponseRuleMap,
        Vec<BridgeDefinitionProjectorResponseRuleMapLsbMsbAttribute>
    )
} 

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum BridgeDefinitionCommandDefinitionOutputProjectorResponseRuleMap {
    LsbMsb
}

#[derive(Deserialize, Debug)]
pub struct BridgeDefinitionProjectorResponseRuleMapLsbMsbAttribute {
    pub rule_type: String,
    pub value: Vec<u8>    
}


#[derive(Deserialize)]
#[derive(Debug)]
pub struct BridgeDefinitionCommandDefinitionOutput {
    #[serde(flatten)]
    pub on_received: BridgeDefinitionCommandDefinitionOutputProjectorResponse,
    #[serde(flatten)]
    pub response: BridgeDefinitionCommandDefinitionOutputResponse,
}

pub struct Error {
    pub message: String
}

impl BridgeDefinition {
    pub fn from_file(file_name: String) -> Result<BridgeDefinition, Error> {
        match fs::read_to_string(file_name) {
            Ok(file_content) => Self::parse_content(file_content),
            Err(err) => Err(Error {message: err.to_string()})
        }
    }

    fn parse_content(file_content: String) -> Result<BridgeDefinition, Error> {
        match toml::from_str::<BridgeDefinition>(&file_content) {
            Ok(content) => Ok(content),
            Err(err) => Err(Error {message: err.to_string()})
        }
    }
}

// #region Serde custom deserialization
#[inline(always)]
fn cast_command_bytes(input: &[u8]) -> [u8; 5] {
    input.try_into().unwrap_or_default()
}

struct BridgeDefinitionCommandsMapVisitor;
impl<'de> Visitor<'de> for BridgeDefinitionCommandsMapVisitor {
    type Value = BridgeDefinitionCommandsMap;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a hashmap with [u8;5] as key, and commands as value")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut map = BridgeDefinitionCommandsMap::with_capacity(access.size_hint().unwrap_or(0));

        while let Some((key, value)) = access.next_entry::<String, BridgeDefinitionCommand>()? {
            let key_bytes = key.as_bytes();

            if key_bytes.len() != 5 {
                return Err(de::Error::custom(format!("commands.{} must have 5 characters", key)));
            }

            map.insert(cast_command_bytes(key_bytes), value);
        }

        Ok(map)
    }
}

struct BridgeDefinitionCommandDefinitionsMapVisitor;
impl<'de> Visitor<'de> for BridgeDefinitionCommandDefinitionsMapVisitor {
    type Value = BridgeDefinitionCommandDefinitionsMap;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a hashmap with Vec<u8> as key, and ouput mappers as value")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut map = BridgeDefinitionCommandDefinitionsMap::with_capacity(access.size_hint().unwrap_or(0));

        while let Some((key, value)) = access.next_entry::<String, BridgeDefinitionCommandDefinition>()? {
            let key_bytes: Vec<u8> = key.into();
            map.insert(key_bytes, value);
        }

        Ok(map)
    }
}

fn deserialize_bridge_commands<'de, D>(deserializer: D) -> Result<BridgeDefinitionCommandsMap, D::Error>
where
    D: Deserializer<'de>, 
{
    deserializer.deserialize_any(BridgeDefinitionCommandsMapVisitor)
}

fn deserialize_bridge_command_definition<'de, D>(deserializer: D) -> Result<BridgeDefinitionCommandDefinitionsMap, D::Error>
where
    D: Deserializer<'de>, 
{
    deserializer.deserialize_any(BridgeDefinitionCommandDefinitionsMapVisitor)
}
// #endregion