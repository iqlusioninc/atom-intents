use reqwest::Client;

pub fn build_http_client() -> Client {
    let builder = Client::builder();
    #[cfg(test)]
    let builder = builder.no_proxy();
    builder.build().expect("http client build failed")
}
