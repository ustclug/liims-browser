use url::Url;

pub fn is_web_url(input: &str) -> bool {
    Url::parse(input).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

pub fn search_url(template: &str, query: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(query.trim().as_bytes()).collect();
    template.replace("%s", &encoded)
}

pub fn resolve_input(input: &str, template: &str) -> Result<String, &'static str> {
    let input = input.trim();
    if input.is_empty() {
        return Err("请输入网址或书名");
    }
    if is_web_url(input) {
        return Ok(input.into());
    }
    // A bare host (including localhost:port and IPv6) is not a URI scheme.
    let candidate = format!("https://{input}");
    let host = input.split('/').next().unwrap_or_default();
    if !input.contains(char::is_whitespace)
        && !input.contains("://")
        && !input.contains('@')
        && (host.contains('.') || host.starts_with("localhost") || host.starts_with('['))
        && is_web_url(&candidate)
    {
        return Ok(candidate);
    }
    if let Some((scheme, _)) = input.split_once(':')
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        return Err("此终端只支持 HTTP 和 HTTPS 网页");
    }
    Ok(search_url(template, input))
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEMPLATE: &str = "https://library.test/search?title=%s&ebook=on";
    #[test]
    fn encodes_queries_without_changing_other_parameters() {
        let result = resolve_input(" Rust & 中文? ", TEMPLATE).unwrap();
        let url = Url::parse(&result).unwrap();
        let params: Vec<_> = url.query_pairs().collect();
        assert_eq!(params[0].1, "Rust & 中文?");
        assert_eq!(params[1].1, "on");
    }
    #[test]
    fn accepts_web_addresses_and_rejects_external_schemes() {
        for uri in [
            "https://example.org/path",
            "http://localhost:8123/",
            "https://[::1]/",
        ] {
            assert_eq!(resolve_input(uri, TEMPLATE).unwrap(), uri);
        }
        assert_eq!(
            resolve_input("example.org/path", TEMPLATE).unwrap(),
            "https://example.org/path"
        );
        for uri in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "mailto:a@b.org",
            "data:text/plain,hi",
            "about:blank",
            "https://user:pass@example.org",
        ] {
            assert!(resolve_input(uri, TEMPLATE).is_err(), "{uri}");
        }
    }
}
