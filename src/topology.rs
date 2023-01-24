use serde::Deserialize;

#[derive(Deserialize)]
pub struct TopologyProducer {
    #[serde(alias="addr")]
    pub address: String,
    pub port: u32,
    pub valency: Option<u32>,
    pub distance: Option<u32>,
    pub continent: Option<String>,
    pub country: Option<String>,
    pub region: Option<String>,
}

#[derive(Deserialize)]
pub struct Topology {
    #[serde(alias="resultcode")]
    pub result_code: String,
    #[serde(alias="networkMagic")]
    pub network_magic: String,
    #[serde(alias="ipType")]
    pub ip_type: u8,
    #[serde(alias="requestedIpVersion")]
    pub requested_ip_version: String,
    #[serde(alias="Producers")]
    pub producers: Vec<TopologyProducer>,
}
