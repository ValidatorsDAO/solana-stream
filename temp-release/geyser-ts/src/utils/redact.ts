// Keep credentials out of logs. Endpoints often carry one in the URL (an `api-key` query
// parameter, a token path segment or `user:password@`), and HTTP client errors print the
// request URL, so log endpoints and such errors through these helpers.

// A URL runs to the next whitespace, quote or angle bracket: commas, parentheses and
// brackets are legal inside a URL (and so inside a credential in it). A trailing run of
// prose punctuation stays outside.
const URL_IN_TEXT = /https?:\/\/[^\s"'`<>]+/gi
const PAIRS: Record<string, string> = { ')': '(', ']': '[', '}': '{' }

const count = (text: string, char: string) => text.split(char).length - 1

/**
 * Length of `token` without the punctuation that follows a URL in prose: a trailing run
 * of `.,;:`, and closing brackets that have no opening one in the URL (the `)` of
 * "(see https://…)"). A balanced bracket belongs to the URL.
 */
const urlLength = (token: string): number => {
  let end = token.length
  while (end > 0) {
    const char = token[end - 1]
    const url = token.slice(0, end)
    const open = PAIRS[char]
    const trailing = '.,;:'.includes(char)
      ? true
      : open !== undefined && count(url, char) > count(url, open)
    if (!trailing) break
    end -= 1
  }
  return end
}

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
  text.replace(URL_IN_TEXT, (match) => {
    const end = urlLength(match)
    return redactUrl(match.slice(0, end)) + match.slice(end)
  })

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
