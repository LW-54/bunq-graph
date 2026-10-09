# bunq-graph

`bunq-graph` synchronizes bunq account balances to a Clepsydra graph. It runs
one synchronization, calculates the exact settlement, prints each transfer,
and submits payments sequentially unless dry-run mode is enabled.

## Runtime Configuration

Build the binary normally:

```bash
cargo build -p bunq-graph
```

Runtime values can be supplied with flags or environment variables:

```bash
bunq-graph \
	--graph ./graph.toml \
	--bindings ./bunq-bindings.toml \
	--installation "$HOME/.local/state/bunq-graph/installation.json" \
	--api-key-file /run/secrets/bunq-api-key
```

The SecretSpec provider can be selected explicitly with
`--secretspec-provider URI`.

The corresponding environment variables are `BUNQ_GRAPH`, `BUNQ_BINDINGS`,
`PATH_TO_BUNQ_INSTALLATION`, `BUNQ_API_KEY_FILE`, `BUNQ_BASE_URL`,
`BUNQ_USER_AGENT`, `BUNQ_DEVICE_DESCRIPTION`, and `BUNQ_DRY_RUN`. `--dry-run`
or `BUNQ_DRY_RUN=1` prints planned transfers without submitting payments.
Command-line flags take precedence over environment values, which take
precedence over compiled SecretSpec values.

The installation JSON is mutable runtime state and is created atomically when
the configured path does not exist. The API key is needed only for initial
installation. The executable exits after bunq accepts the payment requests;
it does not poll balances after submission.

The default build requires a runtime graph supplied with `--graph` or
`BUNQ_GRAPH`. A graph compiled into the executable is an explicit build-time
choice described below.

## Nix Package

The default flake package is runtime-graph-only:

```bash
nix build github:LW-54/bunq-graph
```

The flake exposes `lib.mkBunqGraph` for callers that want to compile their own
graph into the package. The graph is always supplied by the caller; this
repository intentionally contains no committed `graph.toml`:

```nix
{
  inputs.bunq-graph.url = "github:LW-54/bunq-graph";

  outputs = { self, bunq-graph, nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in {
      packages.${system}.default = bunq-graph.lib.mkBunqGraph {
        inherit system;
        embeddedGraph = ./my-graph.toml;
      };
    };
}
```

For inline declarative content, create a store path in the consuming flake:

```nix
embeddedGraph = pkgs.writeText "my-graph.toml" ''
  [nodes.checking]
  [nodes.savings]

  [[flow]]
  source = "checking"
  target = "savings"
  volume = 50
'';
```

An embedded graph changes the derivation and therefore its cache identity. Do
not put secrets in graph files. Publish an embedded build to a shared binary
cache only when its graph is intentionally public. Deployment systems own
their service, scheduling, secret provider, and runtime environment setup.

The package constructor accepts `embeddedGraph = null` by default. The
runtime-only package is also exposed as `packages.<system>.bunq-graph` and
`packages.<system>.default`.

SecretSpec provider features are selected independently from graph embedding:

```nix
bunq-graph.lib.mkBunqGraph {
  system = "x86_64-linux";
  embeddedGraph = ./my-graph.toml;
  secretspecFeatures = [ "sops" "age" ];
}
```

`secretspecFeatures` is a list and defaults to an empty list, which means the
minimal environment-only build. Add any supported provider names to mix and
match them, or use `[ "all-providers" ]` as a shorthand for the complete
supported bundle. The Cargo package exposes an individual `secretspec-*`
feature for every supported non-KDBX provider. The `all-providers` bundle
includes all supported providers except KDBX.

SecretSpec provider selection follows this order:

1. `--secretspec-provider URI`, when supplied.
2. `SECRETSPEC_PROVIDER`, when set.
3. The compile-time package default, configured by `defaultSecretSpecProvider`.
4. `env://`, the bunq-graph fallback.

For example:

```bash
SECRETSPEC_PROVIDER=dotenv://./.env bunq-graph
```

The Nix package default can be changed without changing the source:

```nix
bunq-graph.lib.mkBunqGraph {
  system = "x86_64-linux";
  defaultSecretSpecProvider = "systemd-credential://";
}
```

This sets the compile-time `DEFAULT_SECRETSPEC_PROVIDER` value. It defaults to
`env://` when omitted.

The selected provider must be compiled into the binary. Setting
`SECRETSPEC_PROVIDER` cannot enable a provider that was omitted at compile
time. `SECRETSPEC_PROVIDER` is a whole-resolution override; it does not
preserve per-secret fallback chains.

Only feature-gated providers require an additional compile-time provider
feature.

KDBX is deliberately unavailable in this package. SecretSpec 0.21.1 depends
on `keepass` 0.13.25, whose manifest allows `aes` below 0.9.3 but whose source
uses `cipher` 0.5 traits. Cargo resolves that AES dependency to `aes` 0.8.4,
which implements `cipher` 0.4 traits. Keepass then fails to compile with
missing `BlockCipherEncrypt`, `BlockCipherDecrypt`, and AES constructor
implementations. This is a Keepass dependency-alignment issue, not a
`bunq-api` issue. A newer Keepass release or an upstream Keepass patch aligning
AES and cipher versions is required before KDBX can be supported.

The development shell remains available through devenv:

```bash
direnv allow
```

It uses the devenv nixpkgs channel and its public Cachix cache, while package
builds select their provider features explicitly.

## Schemas

Generate the Bunq Graph and bindings schemas in the current directory:

```bash
nix run github:LW-54/bunq-graph#schemas
```

This writes:

- `bunq-graph.schema.json`
- `bunq-bindings.schema.json`

The graph schema includes the Bunq Graph aliases accepted by Clepsydra, such as
`accounts` for `nodes`, `transfers` for `flow`, and `amount` for `volume`. The
same command refreshes the committed schema files in this repository.
The schemas are intentionally editor-neutral; editors may use them through
their own project settings or schema markers. They do not install extensions,
modify global editor configuration, or assume a particular filename.

To verify the committed schemas are current:

```bash
tmp_dir="$(mktemp -d)"
cargo run -p bunq-graph --bin bunq-graph-schemas -- "$tmp_dir"
cmp bunq-graph.schema.json "$tmp_dir/bunq-graph.schema.json"
cmp bunq-bindings.schema.json "$tmp_dir/bunq-bindings.schema.json"
rm -rf "$tmp_dir"
```

## Sandbox Test

The live tests are disabled by default:

```bash
SANDBOX_TESTS=1 \
cargo test -p bunq-graph --test sandbox_live -- --nocapture
```

To submit the routed sandbox payment as well:

```bash
SANDBOX_TESTS=1 \
SANDBOX_PAYMENTS=1 \
cargo test -p bunq-graph --test sandbox_live -- --nocapture
```

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
nix flake check --override-input devenv-root "file+file://$PWD/.devenv/root"
```

The sandbox tests are skipped unless `SANDBOX_TESTS=1`; payment submission also
requires `SANDBOX_PAYMENTS=1`.

## License

Licensed under either the Apache License, Version 2.0 or the MIT License, at
your option:

- [Apache-2.0](LICENSE-APACHE)
- [MIT](LICENSE-MIT)
