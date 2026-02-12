{ pkgs, ... }: {
  channel = "stable-24.05";

  packages = [
    pkgs.rustup
    pkgs.gcc
  ];

  env = {};

  idx = {
    extensions = [
      "rust-lang.rust-analyzer"
      "tamasfe.even-better-toml"
    ];

    workspace = {
      # Setup was removed to ensure fast startup. 
      # Please run the following command manually in the terminal:
      # rustup default stable
      onCreate = {
        setup-hook = "cp scripts/pre-commit.sh .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit";
      };
      onStart = {
        setup-hook = "cp scripts/pre-commit.sh .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit";
      };
    };
  };
}
