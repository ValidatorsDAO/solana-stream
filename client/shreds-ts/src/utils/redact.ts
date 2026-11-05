// Keep credentials out of logs. Endpoints often carry one in the URL (an `api-key` query
// parameter, a token path segment or `user:password@`), and HTTP client errors print the
// request URL, so log endpoints and such errors through these helpers.

const URL_IN_TEXT = /https?:\/\/[^\s"'`<>()[\]{},]+/gi

/** `scheme://host[:port]`, followed by `/…` when anything after the host was removed. */
export const redactUrl = (url: string): string => {
  try {
    const parsed = new URL(url)
    const rest =
      parsed.username !== '' ||
      parsed.password !== '' ||
      parsed.pathname !== '/' ||
      parsed.search !== '' ||
      parsed.hash !== ''
    return `${parsed.protocol}//${parsed.host}${rest ? '/…' : ''}`
  } catch {
    const scheme = url.indexOf('://')
    return scheme === -1 ? '…' : `${url.slice(0, scheme)}://…`
  }
}

/** `text` with every http(s) URL in it passed through `redactUrl`. */
export const redactUrls = (text: string): string =>
  text.replace(URL_IN_TEXT, (url) => redactUrl(url))

/** An error's message (and its causes') with URLs redacted, for logging. */
export const redactError = (error: unknown): string => {
  const parts: string[] = []
  let current: any = error
  for (let depth = 0; current != null && depth < 8; depth += 1) {
    parts.push(String(current?.message ?? current))
    current = current?.cause
  }
  return redactUrls(parts.join(' <- '))
}

/**
 * A warning when a token would travel unencrypted: set, with an `http://` endpoint that
 * is not this machine.
 */
export const plaintextTokenWarning = (
  endpoint: string,
  token: string | undefined,
): string | null => {
  if (!token) return null
  let parsed: URL
  try {
    parsed = new URL(endpoint)
  } catch {
    return null
  }
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(parsed.hostname)
  if (parsed.protocol !== 'http:' || local) return null
  return `X_TOKEN is sent unencrypted to ${redactUrl(endpoint)}: use an https:// endpoint, or leave X_TOKEN unset if the endpoint authorizes by IP`
}
