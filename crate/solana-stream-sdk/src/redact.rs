//! Keep credentials out of logs.
//!
//! RPC and gRPC endpoints often carry a credential in the URL: an `api-key` query
//! parameter, a token path segment or `user:password@`. HTTP client errors print the
//! request URL (`reqwest` does, and `solana-rpc-client` errors wrap it), so logging an
//! endpoint or such an error as it is publishes the credential to whoever reads the logs.

/// Characters that end a URL embedded in text: whitespace, quotes and angle brackets.
/// Commas, parentheses and brackets are legal inside a URL (and so inside a credential
/// in it), so they do not end it.
fn ends_url(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>')
}

/// Length of `token` without the punctuation that follows a URL in prose: a trailing run
/// of `.,;:`, and closing brackets that have no opening one in the URL (the `)` of
/// "(see https://…)"). A balanced bracket belongs to the URL.
fn url_len(token: &str) -> usize {
    let mut end = token.len();
    while let Some(c) = token[..end].chars().next_back() {
        let unbalanced = |open: char, close: char| {
            let url = &token[..end];
            url.matches(close).count() > url.matches(open).count()
        };
        let trailing = match c {
            '.' | ',' | ';' | ':' => true,
            ')' => unbalanced('(', ')'),
            ']' => unbalanced('[', ']'),
            '}' => unbalanced('{', '}'),
            _ => false,
        };
        if !trailing {
            break;
        }
        end -= c.len_utf8();
    }
    end
}

/// `scheme://host[:port]` of `url`, followed by `/…` when anything after the host was
/// removed (credentials, path, query or fragment).
pub fn redact_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) if parsed.host_str().is_some() => {
            let mut out = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""));
            if let Some(port) = parsed.port() {
                out.push_str(&format!(":{port}"));
            }
            let has_more = !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.path() != "/"
                || parsed.query().is_some()
                || parsed.fragment().is_some();
            if has_more {
                out.push_str("/…");
            }
            out
        }
        _ => match url.find("://") {
            Some(i) => format!("{}://…", &url[..i]),
            None => "…".to_string(),
        },
    }
}

/// `text` with every `http://` and `https://` URL in it passed through [`redact_url`].
///
/// ```
/// use solana_stream_sdk::redact::redact_urls;
///
/// let error = "error sending request for url (https://edge.erpc.global/?api-key=SECRET)";
/// assert_eq!(
///     redact_urls(error),
///     "error sending request for url (https://edge.erpc.global/…)"
/// );
/// ```
pub fn redact_urls(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let rest = &lower[i..];
        let start = match (rest.find("http://"), rest.find("https://")) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let Some(start) = start else {
            out.push_str(&text[i..]);
            break;
        };
        let start = i + start;
        let end = text[start..]
            .char_indices()
            .find(|&(_, c)| ends_url(c))
            .map_or(text.len(), |(offset, _)| start + offset);
        let url_end = start + url_len(&text[start..end]);
        out.push_str(&text[i..start]);
        out.push_str(&redact_url(&text[start..url_end]));
        out.push_str(&text[url_end..end]);
        i = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{redact_url, redact_urls};

    #[test]
    fn removes_credentials_path_and_query_from_urls_in_text() {
        let text = "rpc=https://user:pw@edge.erpc.global:8443/v1/TOKEN?api-key=SECRET#frag, \
                    then \"http://grpc-ams1.erpc.global\" and (HTTPS://Example.com/?k=SECRET2)";
        let redacted = redact_urls(text);
        for secret in ["pw", "TOKEN", "SECRET", "frag"] {
            assert!(!redacted.contains(secret), "{secret} leaked: {redacted}");
        }
        assert_eq!(
            redacted,
            "rpc=https://edge.erpc.global:8443/…, then \"http://grpc-ams1.erpc.global\" and \
             (https://example.com/…)"
        );
        assert_eq!(redact_url("http://127.0.0.1:10000"), "http://127.0.0.1:10000");
        // Commas, parentheses and brackets inside the URL belong to it.
        let tricky = "(https://h.example/?api-key=abc,SEC(RE)T[1]{2}), then https://h.example/p,TOKEN.";
        let redacted = redact_urls(tricky);
        for secret in ["SEC", "RE)T", "TOKEN", "[1]", "{2}"] {
            assert!(!redacted.contains(secret), "{secret} leaked: {redacted}");
        }
        assert_eq!(redacted, "(https://h.example/…), then https://h.example/….");
        assert_eq!(redact_url("https://not a url/?k=SECRET"), "https://…");
    }
}
