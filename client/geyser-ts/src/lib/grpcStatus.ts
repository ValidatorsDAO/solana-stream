// Yellowstone gRPC 17 ends a subscription whose request is larger than the server's
// max_decoding_message_size with RESOURCE_EXHAUSTED and a message that names the limit.
// Reconnecting sends the same request and gets the same answer, so the runner stops.
//
// @triton-one/yellowstone-grpc (napi) reports a stream status as
// Error('subscribe stream receive failed') whose `cause` carries tonic's text:
//   code: 'Some resource has been exhausted', message: "<server message>"
// @grpc/grpc-js errors carry `code: 8` and `details` instead.
const TONIC_RESOURCE_EXHAUSTED = "code: 'Some resource has been exhausted'"
const GRPC_RESOURCE_EXHAUSTED = 8
const QUOTED_MESSAGE = /^, message: ("(?:[^"\\]|\\.)*")/

const unquote = (quoted: string): string => {
  try {
    return JSON.parse(quoted)
  } catch {
    return quoted.slice(1, -1)
  }
}

/** The server's message when `error` is a RESOURCE_EXHAUSTED status, else null. */
export const resourceExhaustedMessage = (error: unknown): string | null => {
  let current: unknown = error
  for (let depth = 0; current != null && depth < 8; depth += 1) {
    const { code, details, message, cause } = current as {
      code?: unknown
      details?: unknown
      message?: unknown
      cause?: unknown
    }
    if (code === GRPC_RESOURCE_EXHAUSTED || code === 'RESOURCE_EXHAUSTED') {
      return String(details ?? message ?? '')
    }
    if (
      typeof message === 'string' &&
      message.startsWith(TONIC_RESOURCE_EXHAUSTED)
    ) {
      const quoted = message
        .slice(TONIC_RESOURCE_EXHAUSTED.length)
        .match(QUOTED_MESSAGE)
      return quoted ? unquote(quoted[1]) : message
    }
    current = cause
  }
  return null
}
