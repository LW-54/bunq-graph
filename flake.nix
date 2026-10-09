{
  description = "bunq account synchronization with Clepsydra graphs";

  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    package-nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";
    devenv-root = {
      url = "file+file:///dev/null";
      flake = false;
    };
    devenv.url = "github:cachix/devenv";
    devenv.inputs.nixpkgs.follows = "nixpkgs";
    nix2container.url = "github:nlewo/nix2container";
    nix2container.inputs.nixpkgs.follows = "nixpkgs";
    mk-shell-bin.url = "github:rrbutani/nix-mk-shell-bin";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  nixConfig = {
    extra-trusted-public-keys = [
      "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw="
    ];
    extra-substituters = [ "https://devenv.cachix.org" ];
  };

  outputs =
    inputs@{ flake-parts, devenv-root, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ inputs.devenv.flakeModule ];

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem = { system, ... }: {
        packages = rec {
          bunq-graph = inputs.self.lib.mkBunqGraph { inherit system; };
          default = bunq-graph;
        };

        checks.bunq-graph = inputs.self.packages.${system}.bunq-graph;

        apps.schemas = {
          type = "app";
          meta.description = "Generate bunq-graph and bindings JSON schemas";
          program = "${inputs.self.packages.${system}.bunq-graph}/bin/bunq-graph-schemas";
        };

        devenv.shells.default = {
          imports = [ ./devenv.nix ];
        };
      };

      flake = {
        lib.mkBunqGraph =
          {
            system ? builtins.currentSystem,
            embeddedGraph ? null,
            secretspecFeatures ? [ ],
            defaultSecretSpecProvider ? "env://",
          }:
          let
            pkgs = import inputs.package-nixpkgs { inherit system; };
            providerFeature = {
              keyring = "secretspec-keyring";
              sops = "secretspec-sops";
              bitwarden = "secretspec-bitwarden";
              keeper = "secretspec-keeper";
              gcsm = "secretspec-gcsm";
              awssm = "secretspec-awssm";
              awsps = "secretspec-awsps";
              vault = "secretspec-vault";
              openbao = "secretspec-openbao";
              bws = "secretspec-bws";
              akv = "secretspec-akv";
              aac = "secretspec-aac";
              infisical = "secretspec-infisical";
              doppler = "secretspec-doppler";
              age = "secretspec-age";
              ejson = "secretspec-ejson";
              scaleway = "secretspec-scaleway";
              kubernetes = "secretspec-kubernetes";
              cloudflare = "secretspec-cloudflare";
              setec = "secretspec-setec";
              all-providers = "secretspec-all-providers";
            };
            selectedFeatures =
              if
                builtins.isList secretspecFeatures
                && builtins.all builtins.isString secretspecFeatures
                && builtins.all (feature: builtins.hasAttr feature providerFeature) secretspecFeatures
              then
                map (feature: providerFeature.${feature}) secretspecFeatures
              else
                throw "secretspecFeatures must be a list of provider names, or [ \"all-providers\" ]";
          in
          pkgs.rustPlatform.buildRustPackage {
            pname = "bunq-graph";
            version = "0.1.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "bunq-api-0.1.0" = "sha256-WSMOGGmpCnW220ImVT3MDAx7RyWkZ00hFjVQB6RQxGw=";
                "clepsydra-lib-0.1.0" = "sha256-xyMLXBSS4TxfqhkuXOdtf1HTPEYcnUJgNfB3/SU0/UI=";
              };
            };
            cargoBuildFlags = [
              "-p"
              "bunq-graph"
            ]
            ++ pkgs.lib.optionals (embeddedGraph != null) [
              "--features"
              "embedded-graph"
            ]
            ++ pkgs.lib.optionals (selectedFeatures != [ ]) [
              "--features"
              (builtins.concatStringsSep "," selectedFeatures)
            ];
            cargoTestFlags = [
              "-p"
              "bunq-graph"
            ];
            nativeBuildInputs = [ pkgs.perl ];
            DEFAULT_SECRETSPEC_PROVIDER = defaultSecretSpecProvider;
            postPatch = pkgs.lib.optionalString (embeddedGraph != null) ''
              cp ${embeddedGraph} crates/bunq-graph/graph.toml
            '';
          };
      };
    };
}
