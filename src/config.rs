use std::{
    env, fmt,
    fs::File,
    io::Read,
    net::{AddrParseError, SocketAddr},
    path::{Path, PathBuf},
};

use thiserror::Error;

pub const DEFAULT_PUBLIC_BIND_ADDR: &str = "0.0.0.0:54871";
pub const DEFAULT_ADMIN_BIND_ADDR: &str = "127.0.0.1:54872";
const DEFAULT_DATABASE_PATH: &str = "/var/lib/aperip-nomos/aperip-nomos.rnmdb";
const MIN_ADMIN_TOKEN_BYTES: usize = 32;
const MAX_ADMIN_TOKEN_BYTES: usize = 256;
const MAX_SECRET_FILE_BYTES: u64 = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigValues {
    pub public_bind_addr: String,
    pub admin_bind_addr: String,
    pub database_path: PathBuf,
    pub page_key_file: PathBuf,
    pub admin_token_file: PathBuf,
}

impl ConfigValues {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            public_bind_addr: resolve_optional_environment(
                "PUBLIC_BIND_ADDR",
                env::var("PUBLIC_BIND_ADDR"),
                DEFAULT_PUBLIC_BIND_ADDR,
            )?,
            admin_bind_addr: resolve_optional_environment(
                "ADMIN_BIND_ADDR",
                env::var("ADMIN_BIND_ADDR"),
                DEFAULT_ADMIN_BIND_ADDR,
            )?,
            database_path: env::var_os("RNMDB_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_DATABASE_PATH)),
            page_key_file: required_path("RNMDB_PAGE_KEY_FILE")?,
            admin_token_file: required_path("ADMIN_TOKEN_FILE")?,
        })
    }
}

#[derive(Eq, PartialEq)]
pub struct ServiceConfig {
    public_bind_addr: SocketAddr,
    admin_bind_addr: SocketAddr,
    database_path: PathBuf,
    page_key: [u8; 32],
    admin_token: String,
}

impl ServiceConfig {
    pub fn load() -> Result<Self, ConfigError> {
        Self::from_values(ConfigValues::from_env()?)
    }

    pub fn from_values(values: ConfigValues) -> Result<Self, ConfigError> {
        let public_bind_addr = parse_address("PUBLIC_BIND_ADDR", &values.public_bind_addr)?;
        let admin_bind_addr = parse_address("ADMIN_BIND_ADDR", &values.admin_bind_addr)?;
        if public_bind_addr.port() == admin_bind_addr.port() {
            return Err(ConfigError::ListenerAddressesMustDiffer);
        }
        let page_key = read_page_key(&values.page_key_file)?;
        let admin_token = read_admin_token(&values.admin_token_file)?;
        Ok(Self {
            public_bind_addr,
            admin_bind_addr,
            database_path: values.database_path,
            page_key,
            admin_token,
        })
    }

    pub fn public_bind_addr(&self) -> SocketAddr {
        self.public_bind_addr
    }

    pub fn admin_bind_addr(&self) -> SocketAddr {
        self.admin_bind_addr
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn page_key(&self) -> [u8; 32] {
        self.page_key
    }

    pub fn admin_token(&self) -> &str {
        &self.admin_token
    }
}

impl fmt::Debug for ServiceConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServiceConfig")
            .field("public_bind_addr", &self.public_bind_addr)
            .field("admin_bind_addr", &self.admin_bind_addr)
            .field("database_path", &self.database_path)
            .field("page_key", &"[REDACTED]")
            .field("admin_token", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("required environment variable is missing: {0}")]
    MissingEnvironment(&'static str),
    #[error("environment variable contains invalid Unicode: {0}")]
    InvalidEnvironment(&'static str),
    #[error("{name} is not a valid socket address: {source}")]
    InvalidAddress {
        name: &'static str,
        #[source]
        source: AddrParseError,
    },
    #[error("public and administrator listeners must use different ports")]
    ListenerAddressesMustDiffer,
    #[error("failed to read secret file {path}: {source}")]
    ReadSecret {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("secret file exceeds {limit} bytes: {path}")]
    SecretTooLarge { path: PathBuf, limit: u64 },
    #[error("RNMDB page key must contain exactly 64 hexadecimal characters")]
    InvalidPageKey,
    #[error("administrator token must contain 32 to 256 visible ASCII characters")]
    InvalidAdminToken,
}

fn required_path(name: &'static str) -> Result<PathBuf, ConfigError> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(ConfigError::MissingEnvironment(name))
}

fn resolve_optional_environment(
    name: &'static str,
    value: Result<String, env::VarError>,
    default: &str,
) -> Result<String, ConfigError> {
    match value {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_string()),
        Err(env::VarError::NotUnicode(_)) => Err(ConfigError::InvalidEnvironment(name)),
    }
}

fn parse_address(name: &'static str, value: &str) -> Result<SocketAddr, ConfigError> {
    value
        .parse()
        .map_err(|source| ConfigError::InvalidAddress { name, source })
}

fn read_page_key(path: &Path) -> Result<[u8; 32], ConfigError> {
    let value = read_secret(path)?;
    let bytes = hex::decode(value).map_err(|_| ConfigError::InvalidPageKey)?;
    bytes.try_into().map_err(|_| ConfigError::InvalidPageKey)
}

fn read_admin_token(path: &Path) -> Result<String, ConfigError> {
    let value = read_secret(path)?;
    let valid = (MIN_ADMIN_TOKEN_BYTES..=MAX_ADMIN_TOKEN_BYTES).contains(&value.len())
        && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte));
    if !valid {
        return Err(ConfigError::InvalidAdminToken);
    }
    Ok(value.to_string())
}

fn read_secret(path: &Path) -> Result<String, ConfigError> {
    let mut file = File::open(path).map_err(|source| read_secret_error(path, source))?;
    let mut bytes = Vec::with_capacity(MAX_SECRET_FILE_BYTES as usize + 1);
    file.by_ref()
        .take(MAX_SECRET_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| read_secret_error(path, source))?;
    if bytes.len() as u64 > MAX_SECRET_FILE_BYTES {
        return Err(ConfigError::SecretTooLarge {
            path: path.to_path_buf(),
            limit: MAX_SECRET_FILE_BYTES,
        });
    }
    let contents = String::from_utf8(bytes).map_err(|error| {
        read_secret_error(
            path,
            std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        )
    })?;
    Ok(contents.trim_matches(char::is_whitespace).to_string())
}

fn read_secret_error(path: &Path, source: std::io::Error) -> ConfigError {
    ConfigError::ReadSecret {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::env::VarError;

    use super::{ConfigError, resolve_optional_environment};

    #[test]
    fn non_unicode_optional_environment_is_rejected() {
        let result = resolve_optional_environment(
            "ADMIN_BIND_ADDR",
            Err(VarError::NotUnicode("invalid".into())),
            "127.0.0.1:54872",
        );

        assert!(matches!(
            result,
            Err(ConfigError::InvalidEnvironment("ADMIN_BIND_ADDR"))
        ));
    }
}
