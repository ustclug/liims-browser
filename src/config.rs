use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};
use url::Url;

pub const DEFAULT_CONFIG: &str = include_str!("../data/browser.toml");

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub idle_seconds: u64,
    pub warning_seconds: u64,
    pub profiles: BTreeMap<String, Profile>,
    #[serde(default)]
    pub site_rules: Vec<SiteRule>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub search_url: String,
    pub links: Vec<Link>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub title: String,
    pub description: String,
    pub icon: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteRule {
    pub host: String,
    #[serde(default = "root_path")]
    pub path_prefix: String,
    #[serde(default)]
    pub input_hint: bool,
    #[serde(default)]
    pub disable_synthetic_bold: bool,
}

fn root_path() -> String {
    "/".into()
}

impl SiteRule {
    pub fn matches(&self, uri: &str) -> bool {
        Url::parse(uri).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str() == Some(self.host.as_str())
                && url.path().starts_with(&self.path_prefix)
        })
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        let default = Path::new("/etc/liims/browser.toml");
        let source = match path {
            Some(path) => {
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?
            }
            None if default.exists() => {
                std::fs::read_to_string(default).map_err(|e| e.to_string())?
            }
            None => DEFAULT_CONFIG.to_owned(),
        };
        Self::parse(&source)
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(source).map_err(|e| e.to_string())?;
        if config.idle_seconds == 0 || config.warning_seconds >= config.idle_seconds {
            return Err("idle_seconds 必须大于 0，warning_seconds 必须小于 idle_seconds".into());
        }
        if !config.profiles.contains_key("default") {
            return Err("缺少 default 校区配置".into());
        }
        for (name, profile) in &config.profiles {
            if profile.name.trim().is_empty() || profile.search_url.matches("%s").count() != 1 {
                return Err(format!(
                    "校区 {name}: 名称不能为空，搜索模板必须有且只有一个 %s"
                ));
            }
            validate_url(&profile.search_url.replace("%s", "test"))?;
            for link in &profile.links {
                validate_url(&link.url)?;
            }
        }
        for rule in &config.site_rules {
            let parsed =
                Url::parse(&format!("https://{}/", rule.host)).map_err(|e| e.to_string())?;
            if parsed.host_str() != Some(&rule.host) || !rule.path_prefix.starts_with('/') {
                return Err("站点规则必须使用精确的小写主机名和以 / 开头的路径".into());
            }
        }
        Ok(config)
    }

    pub fn profile(&self, name: &str) -> Result<Profile, String> {
        self.profiles
            .get(name)
            .cloned()
            .ok_or_else(|| format!("未知校区：{name}"))
    }
}

fn validate_url(uri: &str) -> Result<(), String> {
    if crate::navigation::is_web_url(uri) {
        Ok(())
    } else {
        Err(format!("只允许 HTTP/HTTPS 地址：{uri}"))
    }
}

pub fn boot_profile(cmdline: &str) -> Option<&str> {
    cmdline
        .split_whitespace()
        .find_map(|part| part.strip_prefix("profile="))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_campus_selection() {
        let config = Config::parse(DEFAULT_CONFIG).unwrap();
        assert_eq!(config.profile("default").unwrap().links.len(), 4);
        assert_eq!(config.profile("iat").unwrap().links.len(), 2);
        assert!(config.profile("unknown").is_err());
        assert_eq!(boot_profile("quiet profile=iat splash"), Some("iat"));
        assert_eq!(boot_profile("quiet"), None);
    }
    #[test]
    fn rejects_invalid_settings() {
        assert!(
            Config::parse(&DEFAULT_CONFIG.replace("idle_seconds = 60", "idle_seconds = 0"))
                .is_err()
        );
        assert!(
            Config::parse(&DEFAULT_CONFIG.replace("warning_seconds = 15", "warning_seconds = 60"))
                .is_err()
        );
        assert!(Config::parse(&DEFAULT_CONFIG.replace("title=%s", "title=oops")).is_err());
        assert!(
            Config::parse(
                &DEFAULT_CONFIG.replace("https://email.ustc.edu.cn/", "file:///etc/passwd")
            )
            .is_err()
        );
    }
    #[test]
    fn site_rules_match_origins_not_substrings() {
        let rule = &Config::parse(DEFAULT_CONFIG).unwrap().site_rules[0];
        assert!(rule.matches("http://opac.lib.ustc.edu.cn/opac/search.php"));
        assert!(!rule.matches("https://evil.test/opac.lib.ustc.edu.cn/opac/search"));
        assert!(!rule.matches("https://opac.lib.ustc.edu.cn.evil.test/opac/search"));
        assert!(!rule.matches("https://opac.lib.ustc.edu.cn/elsewhere"));
    }
}
