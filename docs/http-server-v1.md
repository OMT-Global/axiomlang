# HTTP server v1 evidence contract

Issue `#1449` defines the production HTTP server boundary. The checked
`axiom.runtime_http_server.v1` schema records bind authority, request and
response envelopes, bounded handler tasks, backpressure, HTTP/1.1 proxy
behavior, controlled shutdown, and agent-facing evidence. It is a semantic
contract and fixture corpus, not a claim that the production server exists.

## Honest current boundary

The current direct-native subset builds once and serves runtime-origin requests
without generated runtime replay. Under the legacy `net` grant it provides
loopback-only listeners,
method/path/text-body request values, status/text-body responses, a bounded
fixed-route helper, five-second connection timeouts, and explicit server close.
The native subset remains useful evidence and is credited by the
`current-loopback-floor` fixture.

That subset does not qualify external listen authority, query/header/peer or
byte-body request fields, faithful declared response headers, dynamic handler
semantics, structured request tasks, HTTP/1.1 keep-alive, bounded queues,
graceful drain, or observability flushing. The fixed-route helper creates one
host execution context per request and emits HTTP/1.0 close responses. Those
facts keep readiness at `static_spike` / `partial`.

## Server contract

Listen authority is separate from connect authority and denied by default. It
names the exact transport, host, resolved IP, and port. Wildcard or external
binds require an exact grant, are resolved and revalidated at runtime, and may
not silently fall back to a broader or different endpoint.
Trusting forwarded client metadata additionally requires an explicit `proxy`
authority whose transport peer matches a governed network prefix, port, and
transport selector. The matching proxy must remove every client-supplied
`Forwarded` field and emit exactly one field containing exactly one canonical
`for` parameter. Duplicate fields, comma-separated chains, unknown or
obfuscated identifiers, and noncanonical parameters are rejected. The
canonical `for` value is the effective forwarded client identity; the request
`peer` remains the transport-observed proxy. Missing or mismatched proxy
authority discards forwarded metadata and retains the transport-observed peer.

Requests expose method, path, query, ordered headers, transport-observed peer,
and bounded byte/text bodies. Framing and limits are validated before body
allocation. Responses preserve final status values from 200 through 599,
ordered non-framing headers, and byte/text bodies. Header names must satisfy
the HTTP token grammar. The server rejects handler `Content-Length` and
`Transfer-Encoding`, emits its own exact content length, and rejects control
characters in values or a second terminal response. HEAD and 304 suppress body
bytes while allowing a selected-representation length; 204 rejects a handler
body and omits `Content-Length`; 205 rejects a handler body and emits an exact
zero content length.

Each request handler is a structured child task of the server. Connections are
server-owned so a bounded HTTP/1.1 keep-alive connection can serve multiple
requests; each request borrows its connection only for its own structured
scope. Connection, in-flight, accept,
handler, start-line, header, body, read/write/idle, handler, and shutdown limits
are finite. Listener or connection saturation pauses acceptance; because the
application has not accepted that connection, this path cannot emit an HTTP
response. Handler-queue saturation occurs after request acceptance and returns
a bounded `503` without abandoning in-flight handlers. Partial writes retain
progress and wait for readiness or deadline. Unbounded thread-per-connection
fallback is prohibited by the target contract; the current fixed-route spike
still uses one host execution context per accepted request and is not qualified.

The reverse-proxy target is HTTP/1.1 with explicit listen and proxy authorities, an explicit
`max_requests_per_connection` bound plus an idle deadline. The current HTTP/1.0
close-response spike therefore does not claim keep-alive evidence. Conflicting
or incomplete content lengths, Transfer-Encoding plus Content-Length,
unsupported chunked framing, malformed clients, and untrusted forwarded
headers fail closed. Start-line, aggregate header-byte, header-count, and body
ceilings are all fixture-backed. HTTP/2 and WebSockets remain later
work.

Controlled shutdown and SIGTERM move the server through starting, accepting,
draining, and stopped states. Shutdown first stops accepts, drains handlers to
the configured deadline, cancels remaining work, flushes observability, and
closes resources exactly once. Controlled drain, SIGTERM, and server-scope
cancellation are target fixtures only; the current runtime subset does not
implement or claim these lifecycle paths.

## Fixtures and validation

Thirty-three fixtures cover the current runtime floor and loopback-policy denial,
authorized external bind,
dynamic request/response behavior, proxy compatibility, separate listener and
handler-queue saturation, controlled drain, SIGTERM, cancellation, unauthorized
bind, malformed and slow clients, conflicting lengths, TE/CL smuggling,
unsupported chunked framing, start-line/header/body ceilings, proxy-authority
denial,
response-header injection, invalid header names, handler-owned response
framing, incomplete request bodies, authorized-proxy forged-input rewriting,
HEAD/204/205/304 bodyless framing, double response, and prohibited
thread-per-connection fallback. Only `current-loopback-floor` and
`current-loopback-policy-denial` are marked `runtime`; exact Listen Authority
v2 denial and every richer behavior remain `target` evidence. The checker pins
the exact evidence paths and anchors for the current subset and rejects
prose-only fixture drift. A `runtime_complete` claim additionally requires all
fixture references to be promoted to runtime-backed evidence; target-only
fixtures cannot qualify the implementation.

Run the focused contract with:

```bash
python3 scripts/ci/check-http-server-v1.py --root . --json
python3 scripts/ci/test-check-http-server-v1.py
```

The trusted fast PR lane runs its own checker and self-tests while passing the
PR-head checkout through `--root` strictly as data. Production qualification
remains blocked
on issues `#1441`, `#1425`, `#1426`, `#1445`, `#1446`, and `#1447`, followed by
runtime proxy, malformed-client, load, recovery, and shutdown proof on supported
hosts.
