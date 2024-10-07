use std::path::PathBuf;

use serde::Deserialize;
#[derive(Debug, Deserialize)]
pub struct ExecutorConfig {
    pub production: bool,
    pub rpc_url: String,
    pub account_id: String,
}

impl ExecutorConfig {
    pub fn compile_cmd(&self) -> Vec<String> {
        let mut cmd = vec!["run-args".to_string()];

        if self.production {
            cmd.push("--production".to_string());
        }

        cmd.extend_from_slice(&[
            "--rpc-url".to_string(),
            self.rpc_url.clone(),
            "--account-id".to_string(),
            self.account_id.clone(),
            "--key-path".to_string(),
            self.key_path.clone(),
        ]);

        cmd
    }
}

pub fn load_config(path: PathBuf) -> Result<ExecutorConfig, eyre::Report> {
    let config_str = std::fs::read_to_string(path)?;
    let config: ExecutorConfig = serde_yaml::from_str(&config_str)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_relayer_config_compile_cmd() {
        let config = ExecutorConfig {
            production: true,
            rpc_url: "http://example.com".to_string(),
            account_id: "test.near".to_string(),
            key_path: "/path/to/key".to_string()
        };

        let cmd = config.compile_cmd();

        assert_eq!(cmd[0], "run-args");
        assert!(cmd.contains(&"--production".to_string()));
        assert!(cmd.contains(&"--rpc-url".to_string()));
        assert!(cmd.contains(&"http://example.com".to_string()));
        assert!(cmd.contains(&"--da-account-id".to_string()));
        assert!(cmd.contains(&"test.near".to_string()));
    }

    #[test]
    fn test_load_config() -> eyre::Result<()> {
        let config_content = r#"
        production: true
        rpc_url: "http://example.com"
        da_account_id: "test.near"
        key_path: "/path/to/key"
        network: "testnet"
        metrics_ip_port_addr: "127.0.0.1:8080"
        "#;

        let temp_file = NamedTempFile::new()?;
        write!(temp_file.as_file(), "{}", config_content)?;

        let config = load_config(temp_file.path().to_path_buf())?;

        assert!(config.production);
        assert_eq!(config.rpc_url, "http://example.com");
        assert_eq!(config.account_id, "test.near");
        Ok(())
    }
}