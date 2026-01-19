{
  config,
  lib,
  inputs,
  toSource,
  ...
}:
{
  flake-file.inputs.advisory-db = {
    url = "github:rustsec/advisory-db";
    flake = false;
  };

  perSystem =
    { craneLib, pkgs, ... }:
    {
      make-shells.default.packages = [ pkgs.cargo-audit ];
      checks.audit-deps = craneLib.cargoAudit {
        inherit (inputs) advisory-db;

        src =
          [
            config.filesets.manifest
            config.filesets.lockFile
          ]
          |> lib.fileset.unions
          |> toSource;
      };
    };
}
