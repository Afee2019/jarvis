pub mod native;
pub mod traits;

pub use native::NativeRuntime;
pub use traits::RuntimeAdapter;

use crate::config::RuntimeConfig;

/// Factory: create the right runtime from config
pub fn create_runtime(config: &RuntimeConfig) -> anyhow::Result<Box<dyn RuntimeAdapter>> {
    match config.kind.as_str() {
        "native" => Ok(Box::new(NativeRuntime::new())),
        "docker" => anyhow::bail!(
            "runtime.kind='docker' 尚未实现。请使用 runtime.kind='native'，等待容器运行时支持。"
        ),
        "cloudflare" => {
            anyhow::bail!("runtime.kind='cloudflare' 尚未实现。请暂时使用 runtime.kind='native'。")
        }
        other if other.trim().is_empty() => {
            anyhow::bail!("runtime.kind 不能为空。支持的值: native")
        }
        other => anyhow::bail!("未知的运行时类型「{other}」。支持的值: native"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_native() {
        let cfg = RuntimeConfig {
            kind: "native".into(),
        };
        let rt = create_runtime(&cfg).unwrap();
        assert_eq!(rt.name(), "native");
        assert!(rt.has_shell_access());
    }

    #[test]
    fn factory_docker_errors() {
        let cfg = RuntimeConfig {
            kind: "docker".into(),
        };
        match create_runtime(&cfg) {
            Err(err) => assert!(err.to_string().contains("尚未实现")),
            Ok(_) => panic!("docker runtime should error"),
        }
    }

    #[test]
    fn factory_cloudflare_errors() {
        let cfg = RuntimeConfig {
            kind: "cloudflare".into(),
        };
        match create_runtime(&cfg) {
            Err(err) => assert!(err.to_string().contains("尚未实现")),
            Ok(_) => panic!("cloudflare runtime should error"),
        }
    }

    #[test]
    fn factory_unknown_errors() {
        let cfg = RuntimeConfig {
            kind: "wasm-edge-unknown".into(),
        };
        match create_runtime(&cfg) {
            Err(err) => assert!(err.to_string().contains("未知的运行时类型")),
            Ok(_) => panic!("unknown runtime should error"),
        }
    }

    #[test]
    fn factory_empty_errors() {
        let cfg = RuntimeConfig {
            kind: String::new(),
        };
        match create_runtime(&cfg) {
            Err(err) => assert!(err.to_string().contains("不能为空")),
            Ok(_) => panic!("empty runtime should error"),
        }
    }
}
