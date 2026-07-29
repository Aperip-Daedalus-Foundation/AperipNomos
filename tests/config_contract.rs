use std::{fs, net::SocketAddr};

use aperip_nomos::config::{
    ConfigError, ConfigValues, DEFAULT_ADMIN_BIND_ADDR, DEFAULT_PUBLIC_BIND_ADDR, ServiceConfig,
};
use tempfile::tempdir;

fn valid_values(page_key_file: &str, admin_token_file: &str) -> ConfigValues {
    ConfigValues {
        public_bind_addr: "127.0.0.1:8080".to_string(),
        admin_bind_addr: "127.0.0.1:8081".to_string(),
        database_path: "data/aperip-nomos.rnmdb".into(),
        page_key_file: page_key_file.into(),
        admin_token_file: admin_token_file.into(),
    }
}

#[test]
fn defaults_use_large_uncommon_ports() {
    assert_eq!(DEFAULT_PUBLIC_BIND_ADDR, "0.0.0.0:28740");
    assert_eq!(DEFAULT_ADMIN_BIND_ADDR, "127.0.0.1:28741");
}

#[test]
fn loads_distinct_addresses_and_trimmed_secret_files() {
    let directory = tempdir().expect("temporary directory");
    let page_key = directory.path().join("page-key");
    let admin_token = directory.path().join("admin-token");
    fs::write(&page_key, format!("{}\n", "ab".repeat(32))).expect("page key");
    fs::write(&admin_token, format!("{}\r\n", "token-value-".repeat(3))).expect("token");

    let config = ServiceConfig::from_values(valid_values(
        page_key.to_str().expect("UTF-8 path"),
        admin_token.to_str().expect("UTF-8 path"),
    ))
    .expect("valid config");

    assert_eq!(
        config.public_bind_addr(),
        "127.0.0.1:8080".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(
        config.admin_bind_addr(),
        "127.0.0.1:8081".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(config.page_key(), [0xab; 32]);
    assert_eq!(config.admin_token(), "token-value-token-value-token-value-");
    assert!(!format!("{config:?}").contains(config.admin_token()));
}

#[test]
fn rejects_identical_listener_addresses() {
    let directory = tempdir().expect("temporary directory");
    let page_key = directory.path().join("page-key");
    let admin_token = directory.path().join("admin-token");
    fs::write(&page_key, "ab".repeat(32)).expect("page key");
    fs::write(&admin_token, "x".repeat(32)).expect("token");
    let mut values = valid_values(
        page_key.to_str().expect("UTF-8 path"),
        admin_token.to_str().expect("UTF-8 path"),
    );
    values.admin_bind_addr = values.public_bind_addr.clone();

    assert!(matches!(
        ServiceConfig::from_values(values),
        Err(ConfigError::ListenerAddressesMustDiffer)
    ));
}

#[test]
fn rejects_malformed_page_key_and_short_token() {
    let directory = tempdir().expect("temporary directory");
    let page_key = directory.path().join("page-key");
    let admin_token = directory.path().join("admin-token");
    fs::write(&page_key, "not-a-key").expect("page key");
    fs::write(&admin_token, "x".repeat(32)).expect("token");
    assert!(matches!(
        ServiceConfig::from_values(valid_values(
            page_key.to_str().expect("UTF-8 path"),
            admin_token.to_str().expect("UTF-8 path"),
        )),
        Err(ConfigError::InvalidPageKey)
    ));

    fs::write(&page_key, "ab".repeat(32)).expect("page key");
    fs::write(&admin_token, "too-short").expect("token");
    assert!(matches!(
        ServiceConfig::from_values(valid_values(
            page_key.to_str().expect("UTF-8 path"),
            admin_token.to_str().expect("UTF-8 path"),
        )),
        Err(ConfigError::InvalidAdminToken)
    ));
}

#[test]
fn rejects_oversized_secret_before_parsing() {
    let directory = tempdir().expect("temporary directory");
    let page_key = directory.path().join("page-key");
    let admin_token = directory.path().join("admin-token");
    fs::write(&page_key, "a".repeat(2048)).expect("oversized page key");
    fs::write(&admin_token, "x".repeat(32)).expect("token");

    assert!(matches!(
        ServiceConfig::from_values(valid_values(
            page_key.to_str().expect("UTF-8 path"),
            admin_token.to_str().expect("UTF-8 path"),
        )),
        Err(ConfigError::SecretTooLarge { .. })
    ));
}
