use reqwest::Client;

/// Build HTTP client with safety timeouts.
///
/// SECURITY FIX (HTTP): Adds connection and request timeouts to prevent
/// requests from hanging indefinitely.
pub fn build_http_client() -> Client {
    let builder = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10));
    #[cfg(test)]
    let builder = builder.no_proxy();
    builder.build().expect("http client build failed")
}
