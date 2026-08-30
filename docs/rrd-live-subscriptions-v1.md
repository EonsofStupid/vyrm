# RRD durable live subscriptions v1

RRD exposes changefeeds and semantic RRFlowQL live queries through one durable
subscription authority. `POST /v1/subscriptions/open` creates the immutable
stream definition and its delivery policy. The supported Rust client upgrades
`GET /v1/subscriptions/{subscription}/stream` to WebSocket and decodes the
transport-neutral client/server frame enums. `POST /v1/subscriptions/close`
provides an idempotent administrative close path.

## Ordering and replay

The runtime commit cursor is the only delivery order. Changefeed frames retain
each change's commit digest, ordinal, previous-change digest, scope, actor, and
event time. Live-query frames evaluate the same canonical query at the durable
ACK cursor and one captured head, then return deterministic added, updated, and
removed rows. Neither WebSocket connections nor server processes allocate a
second data-order coordinate.

Each subscription durably records its acknowledged cursor. A delivery and its
cursor interval are journaled before the frame is sent. ACKs must name an exact
outstanding delivery in the active connection generation and may cumulatively
acknowledge earlier deliveries. Disconnecting does not advance the ACK. A
reconnect increments the fencing generation, discards the abandoned in-flight
window, and replays from the last durable ACK even after an engine/server
restart. Frames from a replaced generation fail closed.

## Backpressure, leases, and retention

`max_in_flight` bounds the durable unacknowledged delivery window to 1–64
batches. The server stops reading and sending new data while that window is
full; it continues accepting ACKs. `batch_size` retains the existing bounded
changefeed page contract, while live queries retain their separate semantic
delta row and execution budgets.

Every connection has a durable renewable lease. ACK and heartbeat client
frames extend it; an expired lease cannot reconnect or deliver. The configured
cursor-retention window defines `head - window` as the oldest allowed resume
coordinate. Opening, reconnecting, or delivering below that floor returns a
structured failed-precondition error rather than silently skipping changes.
The immutable runtime log remains the authoritative source; this delivery
window is not a second log and cannot rewrite runtime history.

## Authorization and transport

Opening, connecting, acknowledging, and closing have distinct deny-by-default
security actions. Every operation also requires the underlying
`changefeed_follow` or `query_live_poll` grant, so a broad subscription grant
cannot bypass data permission. The HTTP upgrade uses the same session and
bearer headers as ordinary RRD requests. Loopback uses `ws://`; remote service
connections remain behind the existing TLS 1.3 mutual-authentication gate and
use `wss://` with the client's supplied Rustls identity and trust roots.

The WebSocket sends `opened`, `changefeed`, `live_query`, `heartbeat`,
`acknowledged`, `error`, and `closed` frames. Clients send `ack`, `heartbeat`,
and `close`. Binary application frames and malformed JSON are rejected. HTTP
long polling remains a compatibility path over the same runtime/query
authority, not an independent subscription implementation.

## Qualification

Contract tests cover stream and limit validation and schema round trips. Engine
tests cover durable reopen/replay, connection fencing, cumulative ACK,
backpressure, retention expiry, ownership, dual authorization, semantic
live-query deltas, and interleaved independent writers under one global
cursor/hash chain. Real-process client/server tests cover WebSocket push,
unacknowledged disconnect, reconnect replay, ACK, and close.
