use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

pub const BASE_URL: &str = "https://prices.runescape.wiki/api/v2/osrs";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestResponse {
    pub data: HashMap<String, LatestItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestItem {
    pub high: Option<i64>,
    #[serde(rename = "highTime")]
    pub high_time: Option<i64>,
    pub low: Option<i64>,
    #[serde(rename = "lowTime")]
    pub low_time: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingItem {
    pub id: u32,
    pub name: String,
    pub examine: Option<String>,
    pub members: bool,
    pub lowalch: Option<i64>,
    pub highalch: Option<i64>,
    pub limit: Option<i32>,
    pub icon: Option<String>,
}

pub fn build_client(user_agent: &str) -> Client {
    Client::builder()
        .user_agent(user_agent)
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to build HTTP client")
}

pub async fn fetch_latest(client: &Client) -> Result<LatestResponse, reqwest::Error> {
    let url = format!("{}/latest", BASE_URL);
    let res = client.get(&url).send().await?.error_for_status()?;
    let data = res.json::<LatestResponse>().await?;
    Ok(data)
}

pub async fn fetch_mapping(client: &Client) -> Result<Vec<MappingItem>, reqwest::Error> {
    let url = format!("{}/mapping", BASE_URL);
    let res = client.get(&url).send().await?.error_for_status()?;
    let items = res.json::<Vec<MappingItem>>().await?;
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_latest() {
        let json_data = r#"{
            "data": {
                "4151": {
                    "high": 1800000,
                    "highTime": 1700000000,
                    "low": 1790000,
                    "lowTime": 1700000001
                }
            }
        }"#;

        let res: LatestResponse = serde_json::from_str(json_data).unwrap();
        let item = res.data.get("4151").unwrap();
        assert_eq!(item.high, Some(1800000));
        assert_eq!(item.high_time, Some(1700000000));
        assert_eq!(item.low, Some(1790000));
        assert_eq!(item.low_time, Some(1700000001));
    }

    #[test]
    fn test_deserialize_mapping() {
        let json_data = r#"[{
            "id": 4151,
            "name": "Abyssal whip",
            "examine": "A weapon from the abyss.",
            "members": true,
            "lowalch": 72000,
            "highalch": 120000,
            "limit": 70,
            "icon": "Abyssal whip.png"
        }]"#;

        let items: Vec<MappingItem> = serde_json::from_str(json_data).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, 4151);
        assert_eq!(items[0].name, "Abyssal whip");
        assert!(items[0].members);
        assert_eq!(items[0].limit, Some(70));
    }
}
