use hyper::body;
use hyper::header::{ACCEPT as HYPER_ACCEPT, CONTENT_TYPE as HYPER_CONTENT_TYPE};
use hyper::{Body, Client as HyperClient, Request};
use hyper_rustls::HttpsConnectorBuilder;
use isahc::prelude::*;
use reqwest::blocking::Client as ReqwestClient;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::json;
use std::env;

fn env_or(key: &str, fallback: &str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_string())
}

#[derive(Debug)]
struct ProbeResult {
    client: &'static str,
    ok: bool,
    status: u16,
    body: String,
    error: String,
}

fn print_result(result: &ProbeResult) {
    println!("== {} ==", result.client);
    println!("ok={}", result.ok);
    println!("status={}", result.status);
    if !result.error.is_empty() {
        println!("error={}", result.error);
    }
    println!("{}", result.body);
    println!();
}

fn probe_reqwest(url: &str, api_key: &str, payload: &str) -> ProbeResult {
    let client = match ReqwestClient::builder()
        .http1_only()
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            return ProbeResult {
                client: "reqwest",
                ok: false,
                status: 0,
                body: String::new(),
                error: err.to_string(),
            }
        }
    };

    match client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "*/*")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .body(payload.to_string())
        .send()
    {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            ProbeResult {
                client: "reqwest",
                ok: (200..300).contains(&status),
                status,
                body,
                error: String::new(),
            }
        }
        Err(err) => ProbeResult {
            client: "reqwest",
            ok: false,
            status: 0,
            body: String::new(),
            error: err.to_string(),
        },
    }
}

fn probe_isahc(url: &str, api_key: &str, payload: &str) -> ProbeResult {
    let client = match isahc::HttpClient::builder().build() {
        Ok(client) => client,
        Err(err) => {
            return ProbeResult {
                client: "isahc",
                ok: false,
                status: 0,
                body: String::new(),
                error: err.to_string(),
            }
        }
    };

    let request = match isahc::Request::post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "*/*")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .body(payload.to_string())
    {
        Ok(request) => request,
        Err(err) => {
            return ProbeResult {
                client: "isahc",
                ok: false,
                status: 0,
                body: String::new(),
                error: err.to_string(),
            }
        }
    };

    match client.send(request) {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            ProbeResult {
                client: "isahc",
                ok: (200..300).contains(&status),
                status,
                body,
                error: String::new(),
            }
        }
        Err(err) => ProbeResult {
            client: "isahc",
            ok: false,
            status: 0,
            body: String::new(),
            error: err.to_string(),
        },
    }
}

fn probe_hyper(url: &str, api_key: &str, payload: &str) -> ProbeResult {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            return ProbeResult {
                client: "hyper",
                ok: false,
                status: 0,
                body: String::new(),
                error: err.to_string(),
            }
        }
    };

    runtime.block_on(async move {
        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .https_or_http()
            .enable_http1()
            .build();
        let client = HyperClient::builder().build::<_, Body>(https);

        let request = match Request::post(url)
            .header(HYPER_CONTENT_TYPE, "application/json")
            .header(HYPER_ACCEPT, "*/*")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .body(Body::from(payload.to_string()))
        {
            Ok(request) => request,
            Err(err) => {
                return ProbeResult {
                    client: "hyper",
                    ok: false,
                    status: 0,
                    body: String::new(),
                    error: err.to_string(),
                }
            }
        };

        match client.request(request).await {
            Ok(response) => {
                let status = response.status().as_u16();
                let bytes = body::to_bytes(response.into_body())
                    .await
                    .map_err(|err| err.to_string());
                match bytes {
                    Ok(bytes) => ProbeResult {
                        client: "hyper",
                        ok: (200..300).contains(&status),
                        status,
                        body: String::from_utf8_lossy(&bytes).to_string(),
                        error: String::new(),
                    },
                    Err(err) => ProbeResult {
                        client: "hyper",
                        ok: false,
                        status,
                        body: String::new(),
                        error: err,
                    },
                }
            }
            Err(err) => ProbeResult {
                client: "hyper",
                ok: false,
                status: 0,
                body: String::new(),
                error: err.to_string(),
            },
        }
    })
}

fn main() {
    let url = env_or(
        "OPENPUP_ANTHROPIC_URL",
        "https://coding.dashscope.aliyuncs.com/apps/anthropic/v1/messages",
    );
    let api_key = env::var("OPENPUP_ANTHROPIC_API_KEY")
        .expect("OPENPUP_ANTHROPIC_API_KEY is required");
    let model = env_or("OPENPUP_ANTHROPIC_MODEL", "qwen3.6-plus");

    let body = json!({
        "model": model,
        "messages": [
            { "role": "user", "content": "hi" }
        ],
        "max_tokens": 8192,
        "stream": false,
        "system": [
            { "type": "text", "text": "You are a coding assistant." }
        ]
    });
    let payload = body.to_string();

    let results = vec![
        probe_reqwest(&url, &api_key, &payload),
        probe_hyper(&url, &api_key, &payload),
        probe_isahc(&url, &api_key, &payload),
    ];

    for result in &results {
        print_result(result);
    }
}
