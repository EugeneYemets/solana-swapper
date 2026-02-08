use reqwest::header::{HeaderMap, HeaderValue};

pub fn jup_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("Content-Type", HeaderValue::from_static("application/json"));
    h
}
