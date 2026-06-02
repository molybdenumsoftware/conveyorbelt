{
  inputs,
  lib,
  config,
  ...
}:
{
  options = {
    workspaceManifest = lib.mkOption { type = lib.types.anything; };
    binManifest = lib.mkOption { type = lib.types.anything; };
  };

  config = {
    flake-file.inputs.crane.url = "github:ipetkov/crane";

    binManifest = {
      package = lib.genAttrs [ "edition" "license" ] (_name: {
        workspace = true;
      });
    };

    workspaceManifest.workspace = {
      resolver = "3";
      package.edition = "2024";
    };

    perSystem =
      { pkgs, ... }:
      {
        options = {
          buildArgs = lib.mkOption { type = lib.types.lazyAttrsOf lib.types.anything; };
          buildEnv = lib.mkOption { type = lib.types.lazyAttrsOf lib.types.str; };
          checkEnv = lib.mkOption { type = lib.types.lazyAttrsOf lib.types.str; };
        };

        config = {
          buildArgs.strictDeps = true;
          _module.args.craneLib = inputs.crane.mkLib pkgs;
          gitignore = [ "/target" ];

          files.files = [
            {
              path_ = "Cargo.toml";
              drv = pkgs.writers.writeTOML "Cargo.toml" config.workspaceManifest;
            }
            {
              path_ = "crates/bin/Cargo.toml";
              drv = pkgs.writers.writeTOML "Cargo.toml" config.binManifest;
            }
          ];

          make-shells.default = {
            packages = [
              pkgs.rust-analyzer
              pkgs.rustfmt
            ];
            env.RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
        };
      };
  };
}
