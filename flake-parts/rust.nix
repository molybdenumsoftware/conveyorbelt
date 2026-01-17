{
  inputs,
  lib,
  config,
  ...
}:
{
  options.cargoManifest = lib.mkOption { type = lib.types.anything; };

  config = {
    flake-file.inputs.crane.url = "github:ipetkov/crane";

    cargoManifest.package.edition = "2024";

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
              drv = pkgs.writers.writeTOML "Cargo.toml" config.cargoManifest;
            }
          ];
        };
      };
  };
}
