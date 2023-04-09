use bitcoin::Network;

pub struct MockBlockchainProvider {
    host: String,
    network: Network,
}

impl MockBlockchainProvider {
    pub fn new(host: String, network: Network) -> Self {
        Self { host, network }
    }
}
