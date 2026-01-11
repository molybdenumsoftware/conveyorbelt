{
  config,
  lib,
  toSource,
  rootPath,
  ...
}:
{
  cargoManifest.bin = [
    {
      name = config.metadata.title;
      path = "src/main.rs";
    }
  ];

  perSystem =
    { craneLib, pkgs, ... }:
    let
      path_ = "deny.toml";
    in
    {
      files.files = [
        {
          inherit path_;
          drv = pkgs.writers.writeTOML "deny.toml" {
            licenses.allow = [
              "Apache-2.0"
              "MIT"
              "Unicode-3.0"
              "Zlib"
            ];
          };
        }
      ];
      checks.cargo-deny = craneLib.cargoDeny {
        src =
          [
            config.filesets.manifest
            config.filesets.lockFile
            (rootPath + "/${path_}")
          ]
          |> lib.fileset.unions
          |> toSource;
      };
    };
}
