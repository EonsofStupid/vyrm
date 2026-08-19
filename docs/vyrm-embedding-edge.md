# Vyrm embedding, compact artifact, accelerator, and edge contract (M6)

Status: local executable kernel gate, 2026-08-19.

M6 closes the gap between canonical source bytes and a locally searchable,
model-bound vector artifact. It does not make inference or an ANN index
authoritative: source identity, model identity, transaction freshness, and the
canonical runtime log remain the admission boundary.

## Embedding jobs

`vyrm-embed` defines a provider-neutral `EmbeddingJob` containing the source
reference and expected SHA-256, exact read stamp, target/subject/field,
valid-time window, complete model identity and digest, execution/network
policy, and canonical job digest. `EmbeddingCoordinator::prepare`:

1. validates the backend capability and exact requested model before reading;
2. denies a network-requiring backend when the job says `deny`;
3. reads and authenticates source bytes;
4. performs inference outside a storage transaction;
5. reads the source again and rejects an inference-time change;
6. produces a `RuntimeVector` with source/model/job/backend provenance; and
7. constructs a `DataTransaction` bound to the original `ReadStamp`, so a
   later source/runtime mutation loses compare-and-swap at commit.

The built-in `FeatureHashBackend` is a deterministic, dependency-free offline
pipeline baseline. It is not presented as a semantic-quality model.

The optional `fastembed-local` feature adds a local ONNX adapter using
FastEmbed 5.17.4 with its hub, TLS, and model-downloader features disabled.
Callers supply ONNX, tokenizer, external-initializer, and runtime-configuration
bytes. A length-framed SHA-256 over all of that material becomes model
provenance. The adapter cannot select a remote model. This follows
[FastEmbed's user-defined model path](https://docs.rs/fastembed/5.17.4/fastembed/struct.UserDefinedEmbeddingModel.html)
while making the no-network boundary a build feature rather than a cache
assumption.

## Model-bound search

`EmbeddingModelBinding { name, digest }` is now optional on legacy vector
requests and projection configurations and mandatory in the M6 edge path.
Exact search admits only candidates from the requested embedding space;
compact and HNSW builds reject candidates from another model; the planner
rejects a projection whose binding differs from the query. Equal dimensions do
not imply compatible semantics.

## Compact dense artifact

`CompactDenseSegment` format v1 replaces repeated JSON vector arrays with:

- a fixed 128-byte, explicitly little-endian header;
- canonical JSON identity, temporal, provenance, and payload metadata;
- a 64-byte-aligned row-major `f32` region with zeroed padding;
- a domain-separated SHA-256 tree digest over header and payload; and
- strict length, offset, count, shape, finite-value, canonical-order, padding,
  model, digest, scope, cursor, and freshness validation.

Publication writes a unique staging file, synchronizes it, atomically links it
into place only if absent, synchronizes the directory, and reopens/verifies the result. The
read path supports owned bytes and a genuine read-only `memmap2` mapping.
Scalar byte-decoding is the reference kernel; x86-64 AVX2 is selected only
after runtime feature detection and is differentially checked for ranking and
bounded score error across cosine, dot, Euclidean, and Manhattan metrics.

The format is exact and CPU-loadable regardless of how it is built. Under the
`accelerator` feature, an optional builder declares its GPU platform/device and
supported format. Its output is treated as untrusted bytes, reopened, and
required to be byte-identical to the deterministic CPU artifact before
publication. Corrupt, wrong-generation, wrong-target, failed, or unavailable
builders either fail or take an explicitly policy-authorized CPU fallback.
This boundary is informed by Qdrant's optional GPU HNSW build and cuVS's
CPU-loadable HNSW serialization; it is not evidence that a physical GPU ran in
this environment.

## Offline edge profile and evidence

`vyrm-edge` packages local generation plus mmap exact search without an HTTP
client, async runtime, cluster service, S3 SDK, GPU runtime, model hub, or TLS
dependency. Its CLI builds an artifact from JSON documents and embeds a text
query plus searches it in one local call:

```bash
vyrm-edge build documents.json vectors.vyrdense 128 7
vyrm-edge query vectors.vyrdense "runtime freshness" 10 128 7
```

Reproduce the retained fixed-seed profile with:

```bash
cargo build --locked --release -p vyrm-edge
cargo run --locked --release -p vyrm-edge --example edge_evidence -- \
  /tmp/vyrm-m6-edge-current-10000x128.vyrdense 10000 128
cargo tree --locked -p vyrm-edge --edges normal
```

Retained evidence:
[`evidence/m6-edge-local-10000x128.json`](evidence/m6-edge-local-10000x128.json).
On the same 8-vCPU Xeon/KVM host used for M5, the 10k×128 profile observed a
1,343,624-byte release binary, 12,399,360-byte artifact for 5,120,000 raw vector
bytes (2.42175×), 42,696 KiB fresh-query peak RSS, and 10.98 ms query p95 over
25 exact queries. `/usr/bin/time` reported zero socket messages. These are
single-machine regression baselines, not universal limits or a competitor
comparison.

CI runs all workspace features so the accelerator and local FastEmbed adapters
cannot silently rot, denies common networking stacks from the edge dependency
tree, enforces a 2 MiB release-binary ceiling, and validates the checked-in
artifact/RSS/latency evidence budgets.

## Honest boundary

- The edge CLI intentionally uses FeatureHash to keep CI deterministic and
  model-free. A real FastEmbed/ONNX model must be supplied and evaluated before
  making semantic-quality claims.
- AVX2 ran on this host; non-x86 targets currently use the scalar kernel.
- The accelerator contract and adversarial parity fixture are executable, but
  no CUDA/Vulkan/cuVS device adapter or physical-GPU benchmark is certified.
- The compact format covers dense exact payloads. Sparse/multivector compact
  layouts, payload bitmap indexes, compact HNSW graph storage, and background
  optimization remain open.
- This gate does not establish superiority over Qdrant. A fixed-hardware,
  equivalent-workload comparison belongs after the remaining production paths
  exist.
