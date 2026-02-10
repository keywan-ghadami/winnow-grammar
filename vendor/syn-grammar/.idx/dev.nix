{ pkgs, ... }: {
  # Nutze stable oder unstable (unstable hat oft aktuelleres Rust)
  channel = "unstable";

  packages = [
    pkgs.rustup
    # KRITISCH: gcc liefert den Linker (cc), ohne den Proc Macros fehlschlagen
    pkgs.gcc
  ];

  idx = {
    extensions = [
      "rust-lang.rust-analyzer"
      "tamasfe.even-better-toml"
    ];

    workspace = {
      onCreate = {
        rust-install = "rustup toolchain install stable --profile minimal --component clippy,rustfmt,rust-src,rust-analyzer && rustup default stable";
      };
    };
  };
}
