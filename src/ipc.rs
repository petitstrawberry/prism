use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum CommandRequest {
    Help,
    Clients,
    List,
    Set {
        pid: i32,
        #[serde(alias = "channel_offset")]
        offset: u32,
    },
    Quit,
    Exit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse<T> {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfoPayload {
    pub pid: i32,
    pub client_id: u32,
    pub channel_offset: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsible_pid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsible_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingUpdateAck {
    pub pid: i32,
    pub channel_offset: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPropertyPayload {
    pub selector: u32,
    pub property_data_type: u32,
    pub qualifier_data_type: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelpEntry {
    pub command: String,
    pub usage: String,
    pub description: String,
}

impl HelpEntry {
    pub fn new(
        command: impl Into<String>,
        usage: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            command: command.into(),
            usage: usage.into(),
            description: description.into(),
        }
    }
}
