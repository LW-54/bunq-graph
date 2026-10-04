{ pkgs, ... }: {
  languages.rust = {
    enable = true;
    channel = "stable";
    components = [
      "rustc"
      "cargo"
      "clippy"
      "rustfmt"
      "rust-analyzer"
    ];
  };

  packages = with pkgs; [
    bacon
    cargo-expand
    cargo-nextest
    cargo-deny
    taplo
    nixd
    nixfmt
  ];

  pre-commit.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
    taplo.enable = true;
    nixfmt.enable = true;
    trim-trailing-whitespace.enable = true;
  };
}
